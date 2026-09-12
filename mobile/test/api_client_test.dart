import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:iggybilly/src/api/client.dart';

import 'fakes.dart';

IggybillyApi apiWith(FakeHttpClient http, {String? token = 'tok'}) =>
    IggybillyApi(
      baseUrl: Uri.parse('https://example.test'),
      httpClient: http,
      token: token,
    );

void main() {
  test('every authenticated request carries the bearer token', () async {
    final fake = FakeHttpClient.json(<dynamic>[]);
    await apiWith(fake).clips();
    expect(fake.lastRequest.headers['Authorization'], 'Bearer tok');
  });

  test('signing in does not send a token it does not have yet', () async {
    final fake = FakeHttpClient.json({
      'token': 'fresh',
      'tokenId': 3,
      'user': userJson(),
    });
    final result = await IggybillyApi(
      baseUrl: Uri.parse('https://example.test'),
      httpClient: fake,
    ).signIn(username: 'alice', password: 'pw', deviceName: 'Phone');

    expect(result.token, 'fresh');
    expect(result.user.username, 'alice');
    expect(fake.lastRequest.headers.containsKey('Authorization'), isFalse);
  });

  test('label filters become one query parameter each', () async {
    final fake = FakeHttpClient.json(<dynamic>[]);
    await apiWith(fake).clips(labels: ['verse', 'live take']);
    // Uri.replace(queryParameters:) cannot express a repeated key, which
    // is exactly what the AND filter needs.
    expect(fake.lastRequest.url.query, 'label=verse&label=live+take');
    expect(
      fake.lastRequest.url.queryParametersAll['label'],
      ['verse', 'live take'],
    );
  });

  test('a clip without peaks is parsed, not rejected', () async {
    final fake = FakeHttpClient.json(clipJson(peaks: null, durationSeconds: null));
    final clip = await apiWith(fake).clip(1);
    expect(clip.peaks, isNull);
    expect(clip.duration, isNull);
    expect(clip.name, 'riff');
  });

  test('a fractional duration survives the trip', () async {
    final fake = FakeHttpClient.json(clipJson(durationSeconds: 12.5));
    final clip = await apiWith(fake).clip(1);
    expect(clip.duration, const Duration(milliseconds: 12500));
  });

  test('timestamps arrive as instants and are shown in local time', () async {
    final fake = FakeHttpClient.json(
      clipJson(uploadedAt: '2026-05-26T10:30:00.000Z'),
    );
    final clip = await apiWith(fake).clip(1);
    expect(clip.uploadedAt.isUtc, isFalse, reason: 'converted for the device');
    expect(
      clip.uploadedAt.toUtc(),
      DateTime.utc(2026, 5, 26, 10, 30),
    );
  });

  test("the server's own error wording reaches the user", () async {
    final fake = FakeHttpClient((_) => fails(409, 'A clip named “riff” already exists.'));
    await expectLater(
      apiWith(fake).renameClip(2, 'riff'),
      throwsA(isA<ApiException>()
          .having((e) => e.message, 'message', 'A clip named “riff” already exists.')
          .having((e) => e.isConflict, 'isConflict', isTrue)),
    );
  });

  test('a 401 is recognisable so the app can sign itself out', () async {
    final fake = FakeHttpClient((_) => fails(401, 'invalid or revoked token'));
    await expectLater(
      apiWith(fake).me(),
      throwsA(isA<ApiException>().having((e) => e.isUnauthorized, 'isUnauthorized', isTrue)),
    );
  });

  test('a page that is not JSON is reported as a wrong address', () async {
    final fake = FakeHttpClient((_) => http.Response('<html>hello</html>', 200));
    await expectLater(
      apiWith(fake).me(),
      throwsA(isA<ApiException>().having(
        (e) => e.message,
        'message',
        contains('did not answer like an iggybilly server'),
      )),
    );
  });

  test('an unreachable server names the address rather than the socket', () async {
    final fake = FakeHttpClient((_) => throw http.ClientException('failed host lookup'));
    await expectLater(
      apiWith(fake).me(),
      throwsA(isA<ApiException>()
          .having((e) => e.message, 'message', contains('https://example.test'))
          .having((e) => e.statusCode, 'statusCode', isNull)),
    );
  });

  test('a 204 is a success with no body to parse', () async {
    final fake = FakeHttpClient((_) => http.Response('', 204));
    await apiWith(fake).deleteClip(3);
    expect(fake.lastRequest.method, 'DELETE');
    expect(fake.lastRequest.url.path, '/api/v1/clips/3');
  });

  test('wiki pages come back as Markdown source', () async {
    final fake = FakeHttpClient.json({
      'labelId': 4,
      'labelName': 'verse',
      'content': '# Verse\n\n*two bars*',
      'hasContent': true,
      'lastEditedBy': 'alice',
      'lastEditedAt': '2026-05-26T10:30:00.000Z',
    });
    final page = await apiWith(fake).wiki(4);
    expect(page.content, '# Verse\n\n*two bars*');
    expect(page.lastEditedBy, 'alice');
  });

  test('an unwritten wiki page is empty rather than missing', () async {
    final fake = FakeHttpClient.json({
      'labelId': 4,
      'labelName': 'verse',
      'content': '',
      'hasContent': false,
      'lastEditedBy': null,
      'lastEditedAt': null,
    });
    final page = await apiWith(fake).wiki(4);
    expect(page.hasContent, isFalse);
    expect(page.lastEditedAt, isNull);
  });

  test('a relative audio path resolves against the configured server', () {
    final api = apiWith(FakeHttpClient.json(null));
    expect(
      api.resolve('/clips/9/audio').toString(),
      'https://example.test/clips/9/audio',
    );
  });

  test('the player is given headers it can attach itself', () {
    expect(apiWith(FakeHttpClient.json(null)).authHeaders,
        {'Authorization': 'Bearer tok'});
    expect(apiWith(FakeHttpClient.json(null), token: null).authHeaders, isEmpty);
  });

  test('a label list replaces rather than merges', () async {
    final fake = FakeHttpClient.json([
      {'id': 1, 'name': 'verse'},
      {'id': 2, 'name': 'live'},
    ]);
    final labels = await apiWith(fake).addLabel(3, 'Verse');
    expect(labels.map((l) => l.name), ['verse', 'live']);
    expect(jsonDecode(fake.lastRequest.body), {'name': 'Verse'});
  });

  test('suggestions carry the normalised name to create', () async {
    final fake = FakeHttpClient.json({
      'query': 'vers',
      'matches': ['verse'],
      'canCreate': true,
    });
    final s = await apiWith(fake).suggestLabels(query: 'Vers', clipId: 3);
    expect(s.query, 'vers');
    expect(s.canCreate, isTrue);
    expect(fake.lastRequest.url.queryParameters['clipId'], '3');
  });
}
