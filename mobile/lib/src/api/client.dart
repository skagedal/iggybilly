import 'dart:convert';
import 'dart:io' show SocketException;

import 'package:http/http.dart' as http;

import 'models.dart';

/// Something the server said no to, or something the network did.
///
/// The server answers every failure as `{"error": "..."}`, and those
/// messages are written for a person — "A clip named “riff” already
/// exists." — so they are shown as they arrive rather than replaced with
/// something vaguer.
class ApiException implements Exception {
  ApiException(this.message, {this.statusCode});

  /// The failure the user sees.
  final String message;

  /// Null when the request never reached the server.
  final int? statusCode;

  /// Whether this means "sign in again". The app drops its token and
  /// returns to the sign-in screen on these, rather than showing an
  /// error the user can do nothing about.
  bool get isUnauthorized => statusCode == 401;

  /// A name the user asked for that is already taken.
  bool get isConflict => statusCode == 409;

  @override
  String toString() => message;
}

/// The client for one iggybilly server.
///
/// Holds the base URL and the bearer token, and nothing else: it has no
/// opinion about what the app does with what it returns. The `http.Client`
/// comes in by constructor so tests can hand it a fake rather than
/// standing up a server.
class IggybillyApi {
  IggybillyApi({
    required this.baseUrl,
    http.Client? httpClient,
    this.token,
  }) : _http = httpClient ?? http.Client();

  /// Origin of the server, with no trailing slash.
  final Uri baseUrl;
  final http.Client _http;

  /// The token this client authenticates with. Null before sign-in, and
  /// replaced in place when a password change issues a new one.
  String? token;

  /// Resolve a server-relative path — the `audioUrl` on a clip, say —
  /// into an absolute URL for this server.
  Uri resolve(String path) => baseUrl.resolve(path);

  /// The headers a media player needs to fetch audio itself.
  Map<String, String> get authHeaders =>
      token == null ? const {} : {'Authorization': 'Bearer $token'};

  void close() => _http.close();

  // -- Signing in ---------------------------------------------------------

  /// Exchange a password for a token. Does not set [token]: the caller
  /// decides whether this sign-in is the one to keep.
  Future<SignIn> signIn({
    required String username,
    required String password,
    required String deviceName,
  }) async {
    final json = await _send(
      'POST',
      '/api/v1/tokens',
      body: {
        'username': username,
        'password': password,
        'deviceName': deviceName,
      },
      authenticated: false,
    );
    final map = json as Map<String, dynamic>;
    return SignIn(
      token: map['token'] as String,
      user: User.fromJson(map['user'] as Map<String, dynamic>),
    );
  }

  /// Who the current token belongs to. Used on launch to find out
  /// whether a stored token is still good.
  Future<User> me() async =>
      User.fromJson(await _send('GET', '/api/v1/me') as Map<String, dynamic>);

  /// Sign this device out, revoking only its own token.
  Future<void> signOut() => _send('DELETE', '/api/v1/tokens/current');

  Future<List<Device>> devices() async {
    final list = await _send('GET', '/api/v1/tokens') as List<dynamic>;
    return list
        .map((d) => Device.fromJson(d as Map<String, dynamic>))
        .toList(growable: false);
  }

  Future<void> revokeDevice(int id) => _send('DELETE', '/api/v1/tokens/$id');

  /// Change the password. Every token is revoked, so the server hands
  /// back a replacement for this device; the caller must keep it.
  Future<String> changePassword({
    required String currentPassword,
    required String newPassword,
    required String deviceName,
  }) async {
    final json = await _send(
      'POST',
      '/api/v1/me/password',
      body: {
        'currentPassword': currentPassword,
        'newPassword': newPassword,
        'deviceName': deviceName,
      },
    ) as Map<String, dynamic>;
    return json['token'] as String;
  }

  // -- Clips --------------------------------------------------------------

  /// Every clip carrying *all* of [labels]; an empty list is everything.
  Future<List<Clip>> clips({List<String> labels = const []}) async {
    final query = labels.map((l) => MapEntry('label', l));
    final list = await _send(
      'GET',
      '/api/v1/clips',
      query: query.toList(growable: false),
    ) as List<dynamic>;
    return list
        .map((c) => Clip.fromJson(c as Map<String, dynamic>))
        .toList(growable: false);
  }

  Future<Clip> clip(int id) async => Clip.fromJson(
      await _send('GET', '/api/v1/clips/$id') as Map<String, dynamic>);

  Future<void> deleteClip(int id) => _send('DELETE', '/api/v1/clips/$id');

  /// Rename a clip, returning the name it landed on — the server trims,
  /// so what you asked for is not necessarily what you get.
  Future<String> renameClip(int id, String name) async {
    final json = await _send(
      'POST',
      '/api/v1/clips/$id/name',
      body: {'name': name},
    ) as Map<String, dynamic>;
    return json['name'] as String;
  }

  /// Upload audio files. Each becomes its own clip; the server derives
  /// the name from the filename and de-duplicates it.
  Future<List<UploadedClip>> upload(List<UploadFile> files) async {
    final request = http.MultipartRequest('POST', resolve('/api/v1/clips'))
      ..headers.addAll(authHeaders);
    for (final file in files) {
      request.files.add(http.MultipartFile.fromBytes(
        'audio',
        file.bytes,
        filename: file.filename,
      ));
    }

    final http.StreamedResponse streamed;
    try {
      streamed = await _http.send(request);
    } on SocketException catch (e) {
      throw ApiException(_networkMessage(e));
    } on http.ClientException catch (e) {
      throw ApiException(_networkMessage(e));
    }
    final response = await http.Response.fromStream(streamed);
    final json = _decode(response);
    final list = (json as Map<String, dynamic>)['clips'] as List<dynamic>;
    return list
        .map((c) => UploadedClip.fromJson(c as Map<String, dynamic>))
        .toList(growable: false);
  }

  // -- Labels -------------------------------------------------------------

  /// Add a label to a clip. Returns the clip's whole label list, so the
  /// caller replaces its state rather than guessing what the server did
  /// with the name it sent.
  Future<List<Label>> addLabel(int clipId, String name) async =>
      _labels(await _send(
        'POST',
        '/api/v1/clips/$clipId/labels',
        body: {'name': name},
      ));

  Future<List<Label>> removeLabel(int clipId, int labelId) async =>
      _labels(await _send('DELETE', '/api/v1/clips/$clipId/labels/$labelId'));

  Future<LabelSuggestions> suggestLabels({String query = '', int? clipId}) async {
    final params = <MapEntry<String, String>>[
      MapEntry('q', query),
      if (clipId != null) MapEntry('clipId', '$clipId'),
    ];
    return LabelSuggestions.fromJson(
      await _send('GET', '/api/v1/labels/search', query: params)
          as Map<String, dynamic>,
    );
  }

  List<Label> _labels(Object? json) => (json as List<dynamic>)
      .map((l) => Label.fromJson(l as Map<String, dynamic>))
      .toList(growable: false);

  // -- Playlists ----------------------------------------------------------

  /// A label's clips in the order the band has put them in.
  Future<Playlist> playlist(int labelId) async => Playlist.fromJson(
      await _send('GET', '/api/v1/labels/$labelId/playlist')
          as Map<String, dynamic>);

  /// Move a clip to just after [afterClipId] in a label's playlist, or to
  /// the front when that is null. Returns the label's clip ids in the
  /// order they now stand, which the caller applies rather than its own.
  Future<List<int>> reorderPlaylist(
    int labelId,
    int clipId,
    int? afterClipId,
  ) async {
    final json = await _send(
      'POST',
      '/api/v1/labels/$labelId/order',
      body: {'clipId': clipId, 'afterClipId': afterClipId},
    ) as Map<String, dynamic>;
    return (json['order'] as List<dynamic>).cast<int>();
  }

  // -- Label wiki ---------------------------------------------------------

  Future<WikiPage> wiki(int labelId) async => WikiPage.fromJson(
      await _send('GET', '/api/v1/labels/$labelId/wiki')
          as Map<String, dynamic>);

  Future<WikiPage> saveWiki(int labelId, String content) async =>
      WikiPage.fromJson(await _send(
        'POST',
        '/api/v1/labels/$labelId/wiki',
        body: {'content': content},
      ) as Map<String, dynamic>);

  Future<List<WikiRevision>> wikiHistory(int labelId) async {
    final list = await _send('GET', '/api/v1/labels/$labelId/wiki/history')
        as List<dynamic>;
    return list
        .map((r) => WikiRevision.fromJson(r as Map<String, dynamic>))
        .toList(growable: false);
  }

  /// Restore an old revision. It is appended as a new revision, so
  /// nothing between then and now is lost.
  Future<void> restoreWiki(int labelId, int revisionId) =>
      _send('POST', '/api/v1/labels/$labelId/wiki/restore/$revisionId');

  // -- Plumbing -----------------------------------------------------------

  Future<Object?> _send(
    String method,
    String path, {
    Object? body,
    List<MapEntry<String, String>>? query,
    bool authenticated = true,
  }) async {
    var url = resolve(path);
    if (query != null && query.isNotEmpty) {
      // Built by hand rather than with replace(queryParameters:), which
      // cannot express a key repeated for each active label filter.
      final encoded = query
          .map((e) =>
              '${Uri.encodeQueryComponent(e.key)}=${Uri.encodeQueryComponent(e.value)}')
          .join('&');
      url = url.replace(query: encoded);
    }

    final request = http.Request(method, url);
    if (authenticated) request.headers.addAll(authHeaders);
    if (body != null) {
      request.headers['content-type'] = 'application/json';
      request.body = jsonEncode(body);
    }

    final http.StreamedResponse streamed;
    try {
      streamed = await _http.send(request);
    } on SocketException catch (e) {
      throw ApiException(_networkMessage(e));
    } on http.ClientException catch (e) {
      throw ApiException(_networkMessage(e));
    }
    return _decode(await http.Response.fromStream(streamed));
  }

  /// Turn a response into its decoded body, or throw what went wrong.
  Object? _decode(http.Response response) {
    if (response.statusCode == 204 || response.body.isEmpty) {
      if (response.statusCode >= 400) {
        throw ApiException(
          'The server returned ${response.statusCode}.',
          statusCode: response.statusCode,
        );
      }
      return null;
    }

    Object? json;
    try {
      json = jsonDecode(response.body);
    } on FormatException {
      // Not JSON at all — a proxy's error page, or a URL that is not an
      // iggybilly server. Saying so beats "unexpected character".
      throw ApiException(
        response.statusCode >= 400
            ? 'The server returned ${response.statusCode}.'
            : 'That address did not answer like an iggybilly server.',
        statusCode: response.statusCode,
      );
    }

    if (response.statusCode >= 400) {
      final message = json is Map<String, dynamic> ? json['error'] : null;
      throw ApiException(
        message is String && message.isNotEmpty
            ? message
            : 'The server returned ${response.statusCode}.',
        statusCode: response.statusCode,
      );
    }
    return json;
  }

  /// One wording for every way the request failed to arrive. The
  /// underlying messages name host lookups and socket errors, which tell
  /// the user nothing they can act on.
  String _networkMessage(Object error) =>
      "Couldn't reach $baseUrl. Check the address and your connection.";
}

/// A file chosen for upload, already read into memory.
///
/// In memory because the server caps a clip at 10 MB and rejects the
/// rest, so there is nothing here worth streaming.
class UploadFile {
  const UploadFile({required this.filename, required this.bytes});

  final String filename;
  final List<int> bytes;
}
