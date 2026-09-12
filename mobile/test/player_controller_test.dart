import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:iggybilly/src/api/models.dart';
import 'package:iggybilly/src/player/audio_engine.dart';
import 'package:iggybilly/src/player/player_controller.dart';

/// An engine that does nothing but remember what it was told, and let a
/// test push positions and durations at its own pace.
class FakeEngine implements AudioEngine {
  final positionController = StreamController<Duration>.broadcast();
  final playingController = StreamController<bool>.broadcast();
  final durationController = StreamController<Duration?>.broadcast();
  final completionController = StreamController<void>.broadcast();

  final List<Uri> loaded = [];
  Map<String, String> lastHeaders = const {};
  int playCalls = 0;
  int pauseCalls = 0;
  Duration? lastSeek;
  bool stopped = false;
  bool disposed = false;

  /// What `load` reports back, standing in for the platform working the
  /// length out from the stream.
  Duration? reportedDuration;

  /// Set to make `load` fail, as a missing or undecodable file does.
  Object? loadError;

  @override
  Future<Duration?> load(Uri url, {Map<String, String> headers = const {}}) async {
    if (loadError != null) throw loadError!;
    loaded.add(url);
    lastHeaders = headers;
    return reportedDuration;
  }

  @override
  Future<void> play() async {
    playCalls++;
    playingController.add(true);
  }

  @override
  Future<void> pause() async {
    pauseCalls++;
    playingController.add(false);
  }

  @override
  Future<void> seek(Duration position) async => lastSeek = position;

  @override
  Future<void> stop() async => stopped = true;

  @override
  Stream<Duration> get positions => positionController.stream;

  @override
  Stream<bool> get playing => playingController.stream;

  @override
  Stream<Duration?> get durations => durationController.stream;

  @override
  Stream<void> get completions => completionController.stream;

  @override
  Future<void> dispose() async {
    disposed = true;
    await positionController.close();
    await playingController.close();
    await durationController.close();
    await completionController.close();
  }
}

Clip clipFixture({int id = 1, String name = 'riff', Duration? duration}) => Clip(
      id: id,
      name: name,
      originalFilename: '$name.mp3',
      contentType: 'audio/mpeg',
      uploadedAt: DateTime(2026, 5, 26),
      recordingDate: null,
      uploader: 'alice',
      labels: const [],
      peaks: null,
      duration: duration,
      audioPath: '/clips/$id/audio',
      downloadPath: '/clips/$id/audio?download=1',
      canDelete: true,
    );

void main() {
  late FakeEngine engine;
  late PlayerController player;

  setUp(() {
    engine = FakeEngine();
    player = PlayerController(engine: engine);
  });

  tearDown(() => player.dispose());

  test('nothing is loaded to begin with', () {
    expect(player.clip, isNull);
    expect(player.isPlaying, isFalse);
    expect(player.progress, 0);
  });

  test('playing a clip loads it with the auth headers and starts it', () async {
    await player.play(
      clipFixture(),
      Uri.parse('https://example.test/clips/1/audio'),
      headers: {'Authorization': 'Bearer tok'},
    );

    expect(engine.loaded.single.toString(),
        'https://example.test/clips/1/audio');
    expect(engine.lastHeaders, {'Authorization': 'Bearer tok'});
    expect(engine.playCalls, 1);
    expect(player.clip!.id, 1);
  });

  test('pressing play on the loaded clip toggles instead of reloading', () async {
    final clip = clipFixture();
    final url = Uri.parse('https://example.test/clips/1/audio');

    // Whether sound is coming out is reported by the engine's stream, not
    // returned by the call, so each step waits for that to be delivered.
    await player.play(clip, url);
    await pumpEventQueue();
    expect(player.isPlaying, isTrue);

    await player.play(clip, url);
    await pumpEventQueue();

    // The position is what would be lost by reloading, which is why this
    // rule exists at all.
    expect(engine.loaded.length, 1, reason: 'not loaded a second time');
    expect(engine.pauseCalls, 1);
    expect(player.isPlaying, isFalse);

    await player.play(clip, url);
    await pumpEventQueue();
    expect(player.isPlaying, isTrue);
    expect(engine.loaded.length, 1);
  });

  test('playing another clip takes the player over', () async {
    await player.play(clipFixture(id: 1), Uri.parse('https://e.test/1'));
    await player.play(clipFixture(id: 2, name: 'bridge'), Uri.parse('https://e.test/2'));

    expect(engine.loaded.length, 2);
    expect(player.clip!.id, 2);
    expect(player.clip!.name, 'bridge');
  });

  test('the length the platform reports wins over nothing', () async {
    engine.reportedDuration = const Duration(seconds: 30);
    await player.play(clipFixture(), Uri.parse('https://e.test/1'));
    expect(player.duration, const Duration(seconds: 30));
  });

  test("a clip the server couldn't decode still gets a length", () async {
    // No duration from the server, none from load: the platform finds
    // one once it has enough of the stream.
    await player.play(clipFixture(duration: null), Uri.parse('https://e.test/1'));
    expect(player.duration, isNull);

    engine.durationController.add(const Duration(seconds: 42));
    await pumpEventQueue();
    expect(player.duration, const Duration(seconds: 42));
  });

  test('progress is a fraction, and zero while the length is unknown', () async {
    await player.play(clipFixture(duration: null), Uri.parse('https://e.test/1'));
    engine.positionController.add(const Duration(seconds: 5));
    await pumpEventQueue();
    expect(player.progress, 0, reason: 'nothing to be a fraction of yet');

    engine.durationController.add(const Duration(seconds: 20));
    await pumpEventQueue();
    expect(player.progress, closeTo(0.25, 1e-9));
  });

  test('progress never leaves 0..1 even if the position overruns', () async {
    await player.play(
      clipFixture(duration: const Duration(seconds: 10)),
      Uri.parse('https://e.test/1'),
    );
    engine.positionController.add(const Duration(seconds: 12));
    await pumpEventQueue();
    expect(player.progress, 1.0);
  });

  test('seeking maps a fraction onto the clip', () async {
    await player.play(
      clipFixture(duration: const Duration(seconds: 60)),
      Uri.parse('https://e.test/1'),
    );
    await player.seekFraction(0.5);
    expect(engine.lastSeek, const Duration(seconds: 30));
    expect(player.position, const Duration(seconds: 30));
  });

  test('seeking is ignored with nothing loaded', () async {
    await player.seekFraction(0.5);
    expect(engine.lastSeek, isNull);
  });

  test('seeking past either end is clamped', () async {
    await player.play(
      clipFixture(duration: const Duration(seconds: 60)),
      Uri.parse('https://e.test/1'),
    );
    await player.seekFraction(-1);
    expect(engine.lastSeek, Duration.zero);
    await player.seekFraction(4);
    expect(engine.lastSeek, const Duration(seconds: 60));
  });

  test('reaching the end rewinds but keeps the clip loaded', () async {
    await player.play(clipFixture(), Uri.parse('https://e.test/1'));
    engine.positionController.add(const Duration(seconds: 9));
    await pumpEventQueue();

    engine.completionController.add(null);
    await pumpEventQueue();

    expect(player.isPlaying, isFalse);
    expect(player.position, Duration.zero);
    expect(player.clip, isNotNull, reason: 'press play again and it works');
  });

  test('a clip that will not load is unloaded and reported', () async {
    engine.loadError = Exception('404');
    await player.play(clipFixture(), Uri.parse('https://e.test/1'));

    expect(player.clip, isNull, reason: 'no bar for a clip that cannot play');
    expect(player.error, isNotNull);

    player.clearError();
    expect(player.error, isNull);
  });

  test('stopping unloads, for when the loaded clip is deleted', () async {
    await player.play(clipFixture(), Uri.parse('https://e.test/1'));
    await player.stop();
    expect(player.clip, isNull);
    expect(engine.stopped, isTrue);
  });

  test('renaming the loaded clip updates the caption', () async {
    await player.play(clipFixture(id: 5, name: 'old'), Uri.parse('https://e.test/5'));
    player.renamed(5, 'new');
    expect(player.clip!.name, 'new');

    player.renamed(6, 'somebody else');
    expect(player.clip!.name, 'new', reason: 'a different clip is not ours');
  });

  test('listeners are told when the position moves', () async {
    await player.play(clipFixture(), Uri.parse('https://e.test/1'));
    var notifications = 0;
    player.addListener(() => notifications++);

    engine.positionController.add(const Duration(seconds: 1));
    await pumpEventQueue();
    expect(notifications, greaterThan(0));
  });
}
