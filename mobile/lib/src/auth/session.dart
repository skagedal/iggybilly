import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import 'credential_store.dart';

/// Where the app is, as far as signing in goes.
enum SessionStatus {
  /// Looking for a stored token and checking whether it still works.
  restoring,

  /// No usable credential: show the sign-in screen.
  signedOut,

  /// Signed in, with a working token.
  signedIn,
}

/// Who the app is signed in as, and what it talks to.
///
/// One object owns the credential, the server address and the API
/// client, because those three are only ever right together: a token is
/// for one server, and pointing at a different one invalidates it.
///
/// Everything it needs from the platform — the store, the HTTP client,
/// the device's name — arrives by constructor, so the whole thing runs
/// under `flutter test` against fakes.
class Session extends ChangeNotifier {
  Session({
    required this._store,
    this._httpClient,
    Future<String> Function()? deviceName,
    Uri? defaultServer,
  }) : _deviceName = deviceName ?? _unnamedDevice,
       _defaultServer = defaultServer ?? Uri.parse(_fallbackServer);

  /// What a device calls itself when nothing asked the platform. Only
  /// reached in tests; the app passes the real device name in.
  static Future<String> _unnamedDevice() async => 'Mobile app';

  /// Where a fresh install points before anyone types anything. It is
  /// the instance this app was written for; anyone else's is one field
  /// away on the sign-in screen.
  static const _fallbackServer = 'https://iggybilly.skagedal.tech';

  final CredentialStore _store;
  final http.Client? _httpClient;
  final Future<String> Function() _deviceName;
  final Uri _defaultServer;

  SessionStatus _status = SessionStatus.restoring;
  IggybillyApi? _api;
  User? _user;
  Uri? _server;
  String? _lastUsername;

  SessionStatus get status => _status;

  /// The signed-in user, or null when signed out.
  User? get user => _user;

  /// The server the app is pointed at: the stored one, else the default.
  Uri get server => _server ?? _defaultServer;

  /// The username to prefill the sign-in field with.
  String? get lastUsername => _lastUsername;

  /// The band the app is looking at, or null for an account in no band —
  /// which every screen treats as its own state rather than as an empty
  /// list.
  String? get band => _api?.band;

  /// Every band the signed-in user is in, by slug.
  List<String> get bands => _user?.bands ?? const [];

  /// Look at a different band: tell the server, remember it on the
  /// device, and let every screen rebuild against it.
  ///
  /// The server is told first. If that fails the switch does not happen,
  /// which is better than a phone quietly disagreeing with the web about
  /// where it is.
  Future<void> switchBand(String slug) async {
    if (slug == band) return;
    await api.switchBand(slug);
    api.band = slug;
    await _store.writeBand(slug);
    notifyListeners();
  }

  /// The client for the current session. Only valid while signed in;
  /// every screen that uses it is only built in that state.
  IggybillyApi get api {
    final api = _api;
    if (api == null) {
      throw StateError('Session.api read while signed out');
    }
    return api;
  }

  /// Look for a stored token and find out whether it still works.
  ///
  /// A token that the server has revoked — because the device was
  /// removed, or a password was changed — has to be discovered by asking,
  /// so this makes one request. A network failure is *not* treated as a
  /// revoked token: a plane or a tunnel would otherwise sign you out and
  /// lose a credential you still have.
  Future<void> restore() async {
    final storedServer = await _store.readServerUrl();
    _server = storedServer == null ? null : parseServerUrl(storedServer);
    _lastUsername = await _store.readUsername();

    final token = await _store.readToken();
    if (token == null) {
      _set(SessionStatus.signedOut);
      return;
    }

    // The band this device was last looking at, before anything is
    // asked of the network: a cold start in a tunnel opens the band you
    // were in rather than refusing to open at all.
    final api = _newApi(token);
    api.band = await _store.readBand();
    try {
      _user = await api.me();
      // The server has the last word on which bands exist and which of
      // them are this user's, so a stored slug survives only if it is
      // still one of them — someone removed from a band must not keep
      // asking about it.
      final stored = api.band;
      final user = _user!;
      api.band = (stored != null && user.bands.contains(stored))
          ? stored
          : user.band;
      final resolved = api.band;
      if (resolved != null && resolved != stored) {
        await _store.writeBand(resolved);
      }
      _api = api;
      _set(SessionStatus.signedIn);
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await _store.clearToken();
        api.close();
        _set(SessionStatus.signedOut);
      } else {
        // Unreachable, not unauthorised. Keep the token and go in: the
        // screens will show their own "couldn't load" state, and a
        // retry once there is a network costs nothing.
        _api = api;
        _set(SessionStatus.signedIn);
      }
    }
  }

  /// Sign in. Throws [ApiException] with the server's own wording when
  /// the credentials are wrong or the server cannot be reached.
  Future<void> signIn({
    required Uri server,
    required String username,
    required String password,
  }) async {
    final api = IggybillyApi(baseUrl: server, httpClient: _httpClient);
    final SignIn result;
    try {
      result = await api.signIn(
        username: username,
        password: password,
        deviceName: await _deviceName(),
      );
    } catch (_) {
      if (_httpClient == null) api.close();
      rethrow;
    }

    api.token = result.token;
    api.band = result.user.band;
    _server = server;
    _user = result.user;
    _lastUsername = username;
    _api = api;

    await _store.writeServerUrl(server.toString());
    await _store.writeUsername(username);
    await _store.writeToken(result.token);
    final band = result.user.band;
    if (band == null) {
      await _store.clearBand();
    } else {
      await _store.writeBand(band);
    }
    _set(SessionStatus.signedIn);
  }

  /// Sign out, revoking this device's token server-side.
  ///
  /// The local credential is dropped whatever the server says. If the
  /// request fails, the token stays live until it is revoked from
  /// another device — but leaving it on this phone as well would be
  /// worse, and a user who taps sign out has said what they want.
  Future<void> signOut() async {
    final api = _api;
    if (api != null) {
      try {
        await api.signOut();
      } on ApiException {
        // Already revoked, or unreachable. Either way, carry on.
      }
      if (_httpClient == null) api.close();
    }
    await _store.clearToken();
    // The band goes with the sign-out: it is not a secret, but the next
    // person to sign in on this phone has no business landing in the
    // last one's band.
    await _store.clearBand();
    _api = null;
    _user = null;
    _set(SessionStatus.signedOut);
  }

  /// Change the password. Every token is revoked by this, so the server
  /// issues a replacement for this device and it is stored before
  /// anything else can use the old one.
  Future<void> changePassword({
    required String currentPassword,
    required String newPassword,
  }) async {
    final replacement = await api.changePassword(
      currentPassword: currentPassword,
      newPassword: newPassword,
      deviceName: await _deviceName(),
    );
    api.token = replacement;
    await _store.writeToken(replacement);
    notifyListeners();
  }

  /// Drop the session because the server said the token is no longer
  /// good. Screens call this when a request comes back 401 rather than
  /// showing an error the user cannot act on.
  Future<void> handleUnauthorized() async {
    if (_status != SessionStatus.signedIn) return;
    await _store.clearToken();
    final api = _api;
    if (api != null && _httpClient == null) api.close();
    _api = null;
    _user = null;
    _set(SessionStatus.signedOut);
  }

  IggybillyApi _newApi(String token) =>
      IggybillyApi(baseUrl: server, httpClient: _httpClient, token: token);

  void _set(SessionStatus status) {
    _status = status;
    notifyListeners();
  }

  @override
  void dispose() {
    // Only close a client we made. One handed in by a test belongs to
    // the test.
    if (_httpClient == null) _api?.close();
    super.dispose();
  }
}
