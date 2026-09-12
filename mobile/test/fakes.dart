import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:iggybilly/src/auth/credential_store.dart';

export 'package:iggybilly/src/auth/credential_store.dart'
    show InMemoryCredentialStore;

/// An HTTP client that answers from a script instead of a network.
///
/// Records every request so a test can assert on the method, the path,
/// the query and the Authorization header — the last of which is the
/// whole point of the token work, and the easiest thing to get silently
/// wrong.
class FakeHttpClient extends http.BaseClient {
  FakeHttpClient(this.respond);

  /// Given a request, what the server says. Throwing from here is how a
  /// test simulates a network that isn't there.
  final http.Response Function(http.Request request) respond;

  final List<http.Request> requests = [];

  /// Convenience: answer every request with one JSON body and status.
  factory FakeHttpClient.json(Object? body, {int status = 200}) =>
      FakeHttpClient((_) => http.Response(
            jsonEncode(body),
            status,
            headers: {'content-type': 'application/json'},
          ));

  /// Answer by path, so one client can serve a whole flow. Paths are
  /// matched exactly, without the query string.
  factory FakeHttpClient.routes(Map<String, http.Response> routes) =>
      FakeHttpClient((request) =>
          routes[request.url.path] ??
          http.Response('{"error":"no route for ${request.url.path}"}', 404,
              headers: {'content-type': 'application/json'}));

  http.Request get lastRequest => requests.last;

  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) async {
    if (request is http.Request) {
      requests.add(request);
      final response = respond(request);
      return http.StreamedResponse(
        Stream.value(utf8.encode(response.body)),
        response.statusCode,
        headers: response.headers,
        request: request,
      );
    }
    // A multipart upload. Answer with one created clip so the call path
    // can be exercised end to end.
    return http.StreamedResponse(
      Stream.value(utf8.encode('{"clips":[{"id":7,"name":"riff"}]}')),
      200,
      headers: {'content-type': 'application/json'},
      request: request,
    );
  }
}

/// JSON with the right content type, for [FakeHttpClient.routes].
http.Response ok(Object? body) => http.Response(
      jsonEncode(body),
      200,
      headers: {'content-type': 'application/json'},
    );

http.Response fails(int status, String error) => http.Response(
      jsonEncode({'error': error}),
      status,
      headers: {'content-type': 'application/json'},
    );

/// A clip as the server sends one, with the fields a test cares about
/// overridable.
Map<String, dynamic> clipJson({
  int id = 1,
  String name = 'riff',
  String uploader = 'alice',
  bool canDelete = true,
  List<Map<String, dynamic>> labels = const [],
  List<double>? peaks,
  double? durationSeconds,
  String uploadedAt = '2026-05-26T10:30:00.000Z',
  String? recordingDate,
}) =>
    {
      'id': id,
      'name': name,
      'originalFilename': '$name.mp3',
      'contentType': 'audio/mpeg',
      'uploadedAt': uploadedAt,
      'recordingDate': recordingDate,
      'uploader': uploader,
      'labels': labels,
      'peaks': peaks,
      'durationSeconds': durationSeconds,
      'audioUrl': '/clips/$id/audio',
      'downloadUrl': '/clips/$id/audio?download=1',
      'canDelete': canDelete,
    };

Map<String, dynamic> userJson({
  int id = 1,
  String username = 'alice',
  bool isAdmin = false,
}) =>
    {'id': id, 'username': username, 'isAdmin': isAdmin};

/// A credential store that starts out signed in.
InMemoryCredentialStore storeWithToken(String token) =>
    InMemoryCredentialStore(
      token: token,
      serverUrl: 'https://example.test',
      username: 'alice',
    );
