import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:iggybilly/src/auth/session.dart';
import 'package:iggybilly/src/cache/track_cache.dart';
import 'package:iggybilly/src/ui/app.dart';

import 'fakes.dart';
import 'player_controller_test.dart' show FakeEngine;

/// A server that answers the whole app, with enough state to notice a
/// rename or a label going on.
class FakeServer {
  FakeServer({List<Map<String, dynamic>>? clips})
      : clips = clips ??
            [
              clipJson(
                id: 1,
                name: 'riff',
                labels: [
                  {'id': 10, 'name': 'verse'}
                ],
                peaks: [0.1, 0.9, 0.4],
                durationSeconds: 12.0,
              ),
              clipJson(id: 2, name: 'bridge', uploader: 'bob', canDelete: false),
            ];

  final List<Map<String, dynamic>> clips;
  final List<http.Request> seen = [];

  /// Label lists keyed by clip id, as the server would answer an add.
  final Map<int, List<Map<String, dynamic>>> labels = {};

  late final FakeHttpClient client = FakeHttpClient(_respond);

  http.Response _respond(http.Request request) {
    seen.add(request);
    final path = request.url.path;

    if (path == '/api/v1/tokens' && request.method == 'POST') {
      return ok({'token': 'tok', 'tokenId': 1, 'user': userJson()});
    }
    if (path == '/api/v1/tokens') return ok(<dynamic>[]);
    if (path == '/api/v1/me') return ok(userJson());

    if (path == '/api/v1/clips') {
      final filters = request.url.queryParametersAll['label'] ?? const [];
      if (filters.isEmpty) return ok(clips);
      return ok(clips
          .where((c) => filters.every((f) => (c['labels'] as List)
              .any((l) => (l as Map)['name'] == f)))
          .toList());
    }

    final clipMatch = RegExp(r'^/api/v1/clips/(\d+)$').firstMatch(path);
    if (clipMatch != null) {
      final id = int.parse(clipMatch.group(1)!);
      final clip = clips.firstWhere((c) => c['id'] == id);
      return ok(clip);
    }

    final renameMatch = RegExp(r'^/api/v1/clips/(\d+)/name$').firstMatch(path);
    if (renameMatch != null) {
      final id = int.parse(renameMatch.group(1)!);
      final name = (jsonDecode(request.body) as Map)['name'] as String;
      clips.firstWhere((c) => c['id'] == id)['name'] = name.trim();
      return ok({'name': name.trim()});
    }

    final labelMatch = RegExp(r'^/api/v1/clips/(\d+)/labels$').firstMatch(path);
    if (labelMatch != null) {
      final id = int.parse(labelMatch.group(1)!);
      final name = (jsonDecode(request.body) as Map)['name'] as String;
      final list = labels.putIfAbsent(
          id, () => List<Map<String, dynamic>>.from(
              clips.firstWhere((c) => c['id'] == id)['labels'] as List));
      list.add({'id': 99, 'name': name.toLowerCase()});
      clips.firstWhere((c) => c['id'] == id)['labels'] = list;
      return ok(list);
    }

    // Clip audio, which is not under /api/v1: what the player loads and
    // what the cache downloads. Any bytes will do.
    final audioMatch = RegExp(r'^/clips/(\d+)/audio$').firstMatch(path);
    if (audioMatch != null) {
      return http.Response('audio for clip ${audioMatch.group(1)}', 200);
    }

    if (path == '/api/v1/labels/search') {
      return ok({'query': 'chorus', 'matches': <String>[], 'canCreate': true});
    }

    final wikiMatch = RegExp(r'^/api/v1/labels/(\d+)/wiki$').firstMatch(path);
    if (wikiMatch != null) {
      final id = int.parse(wikiMatch.group(1)!);
      if (request.method == 'POST') {
        final content = (jsonDecode(request.body) as Map)['content'] as String;
        return ok(_wiki(id, content));
      }
      return ok(_wiki(id, '# Verse\n\nTwo bars, then the turnaround.'));
    }

    return fails(404, 'no route for $path');
  }

  Map<String, dynamic> _wiki(int id, String content) => {
        'labelId': id,
        'labelName': 'verse',
        'content': content,
        'hasContent': content.isNotEmpty,
        'lastEditedBy': 'alice',
        'lastEditedAt': '2026-05-26T10:30:00.000Z',
      };
}

/// Pump the app with a session wired to [server], already signed in
/// unless [signedIn] says otherwise.
Future<FakeEngine> pumpApp(
  WidgetTester tester,
  FakeServer server, {
  bool signedIn = true,
  TrackCache? cache,
}) async {
  final engine = FakeEngine();
  final session = Session(
    store: signedIn ? storeWithToken('tok') : InMemoryCredentialStore(),
    httpClient: server.client,
    deviceName: () async => 'Test phone',
    defaultServer: Uri.parse('https://example.test'),
  );

  await tester.pumpWidget(
    IggybillyApp(session: session, engine: engine, cache: cache),
  );
  await tester.pumpAndSettle();
  return engine;
}

void main() {
  testWidgets('a signed-out app asks for a server and a password',
      (tester) async {
    await pumpApp(tester, FakeServer(), signedIn: false);

    expect(find.text('Sign in'), findsOneWidget);
    expect(find.widgetWithText(TextFormField, 'Server'), findsOneWidget);
    expect(find.widgetWithText(TextFormField, 'Username'), findsOneWidget);
  });

  testWidgets('signing in lands on the clip list', (tester) async {
    final server = FakeServer();
    await pumpApp(tester, server, signedIn: false);

    await tester.enterText(
        find.widgetWithText(TextFormField, 'Username'), 'alice');
    await tester.enterText(
        find.widgetWithText(TextFormField, 'Password'), 'passw0rd!');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign in'));
    await tester.pumpAndSettle();

    expect(find.text('iggybilly'), findsOneWidget);
    expect(find.text('riff'), findsOneWidget);
    expect(find.text('bridge'), findsOneWidget);
  });

  testWidgets('a wrong password is shown rather than swallowed',
      (tester) async {
    final server = FakeServer();
    final client = FakeHttpClient((_) => fails(401, 'Invalid username or password.'));
    final session = Session(
      store: InMemoryCredentialStore(),
      httpClient: client,
      deviceName: () async => 'Test phone',
      defaultServer: Uri.parse('https://example.test'),
    );
    await tester.pumpWidget(IggybillyApp(session: session, engine: FakeEngine()));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.widgetWithText(TextFormField, 'Username'), 'alice');
    await tester.enterText(
        find.widgetWithText(TextFormField, 'Password'), 'wrong');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign in'));
    await tester.pumpAndSettle();

    expect(find.text('Invalid username or password.'), findsOneWidget);
    expect(server.seen, isEmpty);
  });

  testWidgets('a stored token goes straight to the clips', (tester) async {
    await pumpApp(tester, FakeServer());
    expect(find.text('riff'), findsOneWidget);
  });

  testWidgets('the player bar appears only once something is playing',
      (tester) async {
    final server = FakeServer();
    final engine = await pumpApp(tester, server);

    expect(find.byIcon(Icons.pause), findsNothing);

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();

    // The clip was loaded from the right URL, with the token attached:
    // audio is an authenticated endpoint, not a public one.
    expect(engine.loaded.single.toString(),
        'https://example.test/clips/1/audio');
    expect(engine.lastHeaders['Authorization'], 'Bearer tok');
    expect(engine.playCalls, 1);
  });

  testWidgets('a clip offers to be kept on the phone', (tester) async {
    final server = FakeServer();
    late Directory directory;
    late TrackCache cache;
    // Inside runAsync: a widget test's clock is fake, and real file I/O
    // does not progress under it — an `await` on it outside here never
    // returns. What the switch then *does* is the cache's own tests,
    // which run on a real event loop.
    await tester.runAsync(() async {
      directory = await Directory.systemTemp.createTemp('iggybilly-app');
      cache = TrackCache(
        locate: () async => directory,
        httpClient: server.client,
      );
      await cache.open();
    });
    addTearDown(() => directory.deleteSync(recursive: true));

    await pumpApp(tester, server, cache: cache);
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();

    expect(find.text('Keep downloaded'), findsOneWidget);
    expect(
      find.text('Download it and keep it for offline listening.'),
      findsOneWidget,
    );
  });

  testWidgets('with nowhere to keep clips, nothing offers to', (tester) async {
    final server = FakeServer();
    final cache = TrackCache(
      locate: () async => throw const FileSystemException('no'),
      httpClient: server.client,
    );
    await cache.open();

    await pumpApp(tester, server, cache: cache);
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();

    expect(find.text('Keep downloaded'), findsNothing);
  });

  testWidgets('a clip that will not play says so rather than just vanishing',
      (tester) async {
    final server = FakeServer();
    final engine = await pumpApp(tester, server);
    engine.loadError = Exception('the file is not there');

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();

    expect(find.text('That clip could not be played.'), findsOneWidget);
    expect(find.byIcon(Icons.pause), findsNothing, reason: 'no bar for it');
  });

  testWidgets('tapping the bar opens the player, which can repeat',
      (tester) async {
    final server = FakeServer();
    final engine = await pumpApp(tester, server);

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();

    // The bar's caption; the list row behind it shows the same name, so
    // the last one in the tree is the bar's.
    await tester.tap(find.text('riff').last);
    await tester.pumpAndSettle();

    expect(find.text('Open clip'), findsOneWidget);
    expect(find.byIcon(Icons.replay_10), findsOneWidget);
    expect(find.byIcon(Icons.repeat), findsOneWidget);

    await tester.tap(find.byIcon(Icons.repeat));
    await tester.pumpAndSettle();

    // Looping is the platform's job, and gapless there.
    expect(engine.repeat, isTrue);
    expect(find.byIcon(Icons.repeat_one), findsWidgets);
  });

  testWidgets('the player pane opens the clip it is playing', (tester) async {
    final server = FakeServer();
    await pumpApp(tester, server);

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('riff').last);
    await tester.pumpAndSettle();

    await tester.tap(find.text('Open clip'));
    await tester.pumpAndSettle();

    expect(find.text('Open clip'), findsNothing, reason: 'the pane closed');
    expect(find.text('Uploaded by'), findsOneWidget);
  });

  testWidgets('the bar survives navigating to a clip and back',
      (tester) async {
    final server = FakeServer();
    final engine = await pumpApp(tester, server);

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();

    await tester.tap(find.text('bridge'));
    await tester.pumpAndSettle();
    expect(find.text('Uploaded by'), findsOneWidget);

    // Still the one load: walking to another screen did not restart or
    // tear down the player, which is the whole point of the bar living
    // outside the navigator.
    expect(engine.loaded.length, 1);
    expect(engine.stopped, isFalse);
  });

  testWidgets('a clip page shows what it is and who uploaded it',
      (tester) async {
    await pumpApp(tester, FakeServer());
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();

    expect(find.text('alice'), findsOneWidget);
    expect(find.text('riff.mp3'), findsOneWidget);
    expect(find.text('verse'), findsWidgets);
  });

  testWidgets('only your own clip offers a delete button', (tester) async {
    await pumpApp(tester, FakeServer());

    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.delete_outline), findsOneWidget);

    await tester.pageBack();
    await tester.pumpAndSettle();

    await tester.tap(find.text('bridge'));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.delete_outline), findsNothing,
        reason: 'bob uploaded this one');
  });

  testWidgets('renaming a clip updates the list behind it', (tester) async {
    await pumpApp(tester, FakeServer());
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();

    await tester.tap(find.byIcon(Icons.edit_outlined));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).last, 'main riff');
    await tester.tap(find.widgetWithText(TextButton, 'Rename'));
    await tester.pumpAndSettle();

    expect(find.text('main riff'), findsWidgets);

    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('main riff'), findsOneWidget);
    expect(find.text('riff'), findsNothing);
  });

  testWidgets('tapping a label narrows the list to it', (tester) async {
    await pumpApp(tester, FakeServer());

    // 'riff' carries the label; 'bridge' does not.
    await tester.tap(find.text('verse'));
    await tester.pumpAndSettle();

    expect(find.text('riff'), findsOneWidget);
    expect(find.text('bridge'), findsNothing);
    // And the filter is shown as something you can take off again.
    expect(find.byType(InputChip), findsWidgets);
  });

  testWidgets("a label's notes open from the clip page", (tester) async {
    await pumpApp(tester, FakeServer());
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(InputChip, 'verse'));
    await tester.pumpAndSettle();

    // Rendered from Markdown source, which is what the app is sent.
    expect(find.textContaining('Two bars, then the turnaround.'),
        findsOneWidget);
    expect(find.textContaining('edited by alice'), findsOneWidget);
  });

  testWidgets('a wiki page can be edited and saved', (tester) async {
    final server = FakeServer();
    await pumpApp(tester, server);
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(InputChip, 'verse'));
    await tester.pumpAndSettle();

    await tester.tap(find.byIcon(Icons.edit_outlined));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Rewritten.');
    await tester.tap(find.widgetWithText(TextButton, 'Save'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Rewritten.'), findsOneWidget);
    final saved = server.seen.lastWhere((r) => r.method == 'POST');
    expect(jsonDecode(saved.body), {'content': 'Rewritten.'});
  });

  testWidgets('leaving an edited page asks before discarding',
      (tester) async {
    await pumpApp(tester, FakeServer());
    await tester.tap(find.text('riff'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(InputChip, 'verse'));
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.edit_outlined));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'half a thought');

    await tester.pageBack();
    await tester.pumpAndSettle();

    expect(find.text('Discard changes?'), findsOneWidget);
  });

  testWidgets('a revoked token drops the app back to sign-in', (tester) async {
    // Signed in on launch, then the server stops recognising the token —
    // a device revoked from another phone.
    var revoked = false;
    final client = FakeHttpClient((request) {
      if (revoked) return fails(401, 'invalid or revoked token');
      if (request.url.path == '/api/v1/me') return ok(userJson());
      return ok(<dynamic>[]);
    });
    final store = storeWithToken('tok');
    final session = Session(
      store: store,
      httpClient: client,
      deviceName: () async => 'Test phone',
      defaultServer: Uri.parse('https://example.test'),
    );

    await tester.pumpWidget(IggybillyApp(session: session, engine: FakeEngine()));
    await tester.pumpAndSettle();
    expect(find.text('iggybilly'), findsOneWidget);

    revoked = true;
    await tester.drag(find.byType(CustomScrollView), const Offset(0, 300));
    await tester.pumpAndSettle();

    expect(find.text('Sign in'), findsOneWidget);
    expect(store.token, isNull);
  });

  testWidgets('an unreachable server offers a retry rather than a blank page',
      (tester) async {
    final client = FakeHttpClient((request) {
      if (request.url.path == '/api/v1/me') return ok(userJson());
      throw http.ClientException('offline');
    });
    final session = Session(
      store: storeWithToken('tok'),
      httpClient: client,
      deviceName: () async => 'Test phone',
      defaultServer: Uri.parse('https://example.test'),
    );

    await tester.pumpWidget(IggybillyApp(session: session, engine: FakeEngine()));
    await tester.pumpAndSettle();

    expect(find.text('Try again'), findsOneWidget);
    expect(find.textContaining('https://example.test'), findsOneWidget);
  });

  testWidgets('an empty server says so rather than showing nothing',
      (tester) async {
    await pumpApp(tester, FakeServer(clips: []));
    expect(find.textContaining('No clips yet'), findsOneWidget);
  });

  testWidgets('signing out returns to the sign-in screen and stops playback',
      (tester) async {
    final server = FakeServer();
    final engine = await pumpApp(tester, server);

    await tester.tap(find.byIcon(Icons.play_circle).first);
    await tester.pumpAndSettle();

    await tester.tap(find.byIcon(Icons.person_outline));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Sign out'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Sign out'));
    await tester.pumpAndSettle();

    expect(find.widgetWithText(FilledButton, 'Sign in'), findsOneWidget);
    expect(engine.stopped, isTrue,
        reason: 'audio must not outlive the token that fetched it');
  });
}
