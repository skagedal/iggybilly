import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:iggybilly/src/cache/track_cache.dart';
import 'package:iggybilly/src/player/player_controller.dart';

import 'player_controller_test.dart' show FakeEngine, clipFixture;
import 'track_cache_test.dart' show FakeAudioServer, audioOf;

/// Where the player and the cache meet: whether a clip plays from the
/// phone or from the server, and what happens when the copy on the phone
/// turns out to be no good.
void main() {
  late Directory directory;
  late FakeAudioServer server;
  late TrackCache cache;
  late FakeEngine engine;
  late PlayerController player;

  final url = Uri.parse('https://example.test/clips/1/audio');
  const headers = {'Authorization': 'Bearer tok'};

  setUp(() async {
    directory = await Directory.systemTemp.createTemp('iggybilly-player-cache');
    server = FakeAudioServer();
    server.bodies['/clips/1/audio'] = audioOf(1024);
    cache = TrackCache(locate: () async => directory, httpClient: server);
    await cache.open();
    engine = FakeEngine();
    player = PlayerController(engine: engine, cache: cache);
  });

  tearDown(() async {
    player.dispose();
    if (await directory.exists()) await directory.delete(recursive: true);
  });

  test('a clip already on disk plays from the file, not the network',
      () async {
    await cache.store(clipFixture(), url);
    server.requests.clear();

    await player.play(clipFixture(), url, headers: headers);

    expect(engine.loaded.single.isScheme('file'), isTrue);
    expect(engine.lastHeaders, isEmpty, reason: 'a file needs no bearer token');
    expect(server.requests, isEmpty, reason: 'nothing to stall in a pocket');
    expect(player.clip!.id, 1);
  });

  test('a clip that is not on disk streams, and is fetched for next time',
      () async {
    server.bodies['/clips/2/audio'] = audioOf(512, fill: 2);
    final second = Uri.parse('https://example.test/clips/2/audio');

    await player.play(clipFixture(), url, headers: headers);

    // The first play does not wait for a download: it streams, as the app
    // always did.
    expect(engine.loaded.single, url);
    expect(engine.lastHeaders, headers);

    // The copy is fetched in the background. Asking for it again joins
    // that download rather than starting a second one, which is how a
    // test waits for work the player deliberately did not await.
    await cache.store(clipFixture(), url);
    expect(cache.hasCopy(url), isTrue);

    // And the next play uses it. Via another clip, because pressing play
    // on the loaded one toggles it instead of reloading.
    await player.play(clipFixture(id: 2), second);
    await cache.store(clipFixture(id: 2), second);
    await player.play(clipFixture(), url, headers: headers);

    expect(engine.loaded.last.isScheme('file'), isTrue);
  });

  test('a local copy that will not play falls back to the server', () async {
    await cache.store(clipFixture(), url);
    server.requests.clear();
    // The file is there but the platform refuses it — truncated by a
    // crash, or a container this device turns out not to read.
    engine.loadErrorFor = (source) =>
        source.isScheme('file') ? Exception('cannot decode') : null;

    await player.play(clipFixture(), url, headers: headers);

    expect(player.clip!.id, 1, reason: 'it played in the end');
    expect(engine.loaded.single, url, reason: 'from the server');

    // The unplayable copy was dropped and a fresh one fetched in its
    // place, so this is one slow play rather than a clip that would have
    // failed the same way for ever.
    await cache.store(clipFixture(), url);
    expect(server.requests.map((r) => r.url), contains(url));
    expect(cache.hasCopy(url), isTrue);
  });

  test('a clip nothing can play is still reported', () async {
    engine.loadError = Exception('no');

    await player.play(clipFixture(), url, headers: headers);

    expect(player.clip, isNull);
    expect(player.error, isNotNull);
  });

  test('playing a clip puts it at the back of the eviction queue', () async {
    await cache.setMaxBytes(2048);
    server.bodies['/clips/2/audio'] = audioOf(1024, fill: 2);
    server.bodies['/clips/3/audio'] = audioOf(1024, fill: 3);
    final second = Uri.parse('https://example.test/clips/2/audio');
    final third = Uri.parse('https://example.test/clips/3/audio');

    await cache.store(clipFixture(id: 1), url);
    await cache.store(clipFixture(id: 2), second);

    // Playing 1 again is what saves it: without the touch, it is the
    // oldest thing here and the next arrival would take it.
    await player.play(clipFixture(id: 1), url, headers: headers);
    await cache.store(clipFixture(id: 3), third);

    expect(cache.hasCopy(url), isTrue);
    expect(cache.hasCopy(second), isFalse);
    expect(cache.hasCopy(third), isTrue);
  });
}
