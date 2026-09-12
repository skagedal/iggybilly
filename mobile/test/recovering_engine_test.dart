import 'package:flutter_test/flutter_test.dart';
import 'package:iggybilly/src/player/audio_engine.dart';
import 'package:iggybilly/src/player/recovering_engine.dart';
import 'package:just_audio/just_audio.dart' show PlayerInterruptedException;

import 'player_controller_test.dart' show FakeEngine;

void main() {
  late List<FakeEngine> built;
  late int failuresLeft;
  late RecoveringAudioEngine engine;

  final url = Uri.parse('https://example.test/clips/1/audio');
  final other = Uri.parse('https://example.test/clips/2/audio');

  /// Hands out players, the first [failuresLeft] of which are wedged —
  /// which is what a platform player that has failed once behaves like
  /// for the rest of its life.
  AudioEngine build() {
    final fake = FakeEngine();
    if (failuresLeft > 0) {
      failuresLeft--;
      fake.loadError = Exception('the platform player is wedged');
    }
    built.add(fake);
    return fake;
  }

  setUp(() {
    built = [];
    failuresLeft = 0;
    engine = RecoveringAudioEngine(build);
  });

  test('nothing is asked of the platform until something is played', () async {
    await engine.play();
    await engine.pause();
    await engine.seek(const Duration(seconds: 1));
    await engine.stop();

    expect(built, isEmpty);
  });

  test('a failed load is retried on a new player', () async {
    failuresLeft = 1;

    await engine.load(url);

    expect(built.length, 2, reason: 'the wedged one was thrown away');
    expect(built.first.disposed, isTrue);
    expect(built.last.loaded.single, url);
  });

  test('a clip that will not play still fails, but does not poison the rest',
      () async {
    // Both the original player and its replacement refuse this clip, so
    // the clip is genuinely unplayable and the caller must hear about it.
    failuresLeft = 2;
    await expectLater(engine.load(url), throwsA(isA<Exception>()));

    // The bug: before this class, every later clip failed too and only
    // force-quitting the app helped.
    await engine.load(other);

    expect(built.length, 3);
    expect(built.last.loaded.single, other);
  });

  test('a superseded load is not a broken player', () async {
    // just_audio throws this when a second load takes over from a first.
    // Replacing the player here would destroy the load now in charge.
    final fake = FakeEngine()
      ..loadError = PlayerInterruptedException('superseded');
    engine = RecoveringAudioEngine(() => fake);

    await expectLater(
      engine.load(url),
      throwsA(isA<PlayerInterruptedException>()),
    );
    expect(fake.disposed, isFalse);
  });

  test('listeners keep hearing from the player that replaced the old one',
      () async {
    final heard = <bool>[];
    engine.playing.listen(heard.add);

    await engine.load(url);
    built.last.playingController.add(true);
    await pumpEventQueue();

    // Wedge the player that is now loaded, and load something else.
    built.last.loadError = Exception('wedged');
    await engine.load(other);
    built.last.playingController.add(true);
    await pumpEventQueue();

    expect(built.length, 2);
    // The false in the middle is the replacement itself: nothing is
    // loaded on a brand new player, so nothing is playing on it either.
    expect(heard, [true, false, true]);
  });

  test('repeat is re-applied to a replacement', () async {
    await engine.setRepeat(true);
    // Nothing to tell yet — the player is built on the first load.
    expect(built, isEmpty);

    await engine.load(url);
    await pumpEventQueue();
    expect(built.single.repeat, isTrue);

    built.last.loadError = Exception('wedged');
    await engine.load(other);
    await pumpEventQueue();

    expect(built.length, 2);
    expect(built.last.repeat, isTrue, reason: 'a new player has forgotten it');
  });

  test('disposing takes the current player with it', () async {
    await engine.load(url);
    await engine.dispose();

    expect(built.single.disposed, isTrue);
  });
}
