import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Where the app's sign-in survives a restart.
///
/// An interface rather than the plugin calls themselves, so everything
/// above it can be tested: platform channels do not answer under
/// `flutter test`, and a `Session` that reached for them directly would
/// be untestable for no gain.
abstract class CredentialStore {
  /// The bearer token, or null when signed out.
  Future<String?> readToken();
  Future<void> writeToken(String token);
  Future<void> clearToken();

  /// The server this install talks to, as typed by the user. Kept
  /// separately from the token because it is not a secret and is worth
  /// remembering across a sign-out — nobody wants to retype it.
  Future<String?> readServerUrl();
  Future<void> writeServerUrl(String url);

  /// The username last signed in with, to prefill the field.
  Future<String?> readUsername();
  Future<void> writeUsername(String username);
}

/// The real one: the token in the platform keystore, the rest in
/// preferences.
///
/// The split is the point. A token is a credential, and on a rooted or
/// jailbroken device it should not be sitting in a plist next to the
/// server address; the iOS keychain and the plugin's encrypted Android
/// storage are what that costs.
class SecureCredentialStore implements CredentialStore {
  SecureCredentialStore({FlutterSecureStorage? secure})
      : _secure = secure ??
            const FlutterSecureStorage(
              // Android needs no options here: the plugin encrypts with
              // its own ciphers by default now.
              iOptions: IOSOptions(
                // Readable only once the device has been unlocked at
                // least once since boot, and never restored onto a
                // different phone from a backup.
                accessibility: KeychainAccessibility.first_unlock_this_device,
              ),
            );

  static const _tokenKey = 'iggybilly.token';
  static const _serverKey = 'iggybilly.server';
  static const _usernameKey = 'iggybilly.username';

  final FlutterSecureStorage _secure;

  @override
  Future<String?> readToken() => _secure.read(key: _tokenKey);

  @override
  Future<void> writeToken(String token) =>
      _secure.write(key: _tokenKey, value: token);

  @override
  Future<void> clearToken() => _secure.delete(key: _tokenKey);

  @override
  Future<String?> readServerUrl() async =>
      (await SharedPreferences.getInstance()).getString(_serverKey);

  @override
  Future<void> writeServerUrl(String url) async =>
      (await SharedPreferences.getInstance()).setString(_serverKey, url);

  @override
  Future<String?> readUsername() async =>
      (await SharedPreferences.getInstance()).getString(_usernameKey);

  @override
  Future<void> writeUsername(String username) async =>
      (await SharedPreferences.getInstance()).setString(_usernameKey, username);
}

/// An in-memory store, for tests and for a first run where the platform
/// storage is unavailable.
class InMemoryCredentialStore implements CredentialStore {
  InMemoryCredentialStore({this.token, this.serverUrl, this.username});

  String? token;
  String? serverUrl;
  String? username;

  @override
  Future<String?> readToken() async => token;

  @override
  Future<void> writeToken(String value) async => token = value;

  @override
  Future<void> clearToken() async => token = null;

  @override
  Future<String?> readServerUrl() async => serverUrl;

  @override
  Future<void> writeServerUrl(String value) async => serverUrl = value;

  @override
  Future<String?> readUsername() async => username;

  @override
  Future<void> writeUsername(String value) async => username = value;
}
