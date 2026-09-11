import 'dart:async';

import 'package:audio_session/audio_session.dart';
import 'package:just_audio/just_audio.dart';

/// The part of playback that is the platform's.
///
/// An interface, because everything interesting about the player — what
/// happens when you press play on the clip that is already loaded, what
/// the progress bar reads while nothing is loaded — is logic, and logic
/// that reaches a platform channel directly cannot be tested. See
/// [PlayerController], which is the logic, and knows only this.
abstract class AudioEngine {
  /// Point the engine at a clip. Returns the length when the platform
  /// could work it out from the stream, which is how a clip the server
  /// failed to decode still gets a usable progress bar.
  Future<Duration?> load(Uri url, {Map<String, String> headers = const {}});

  Future<void> play();
  Future<void> pause();
  Future<void> seek(Duration position);
  Future<void> stop();

  /// Where playback has got to. Emits often enough to move a bar.
  Stream<Duration> get positions;

  /// Whether sound is actually coming out.
  Stream<bool> get playing;

  /// The clip's length, once known. Null until the platform says.
  Stream<Duration?> get durations;

  /// Fires when a clip reaches its end.
  Stream<void> get completions;

  Future<void> dispose();
}

/// The real engine.
class JustAudioEngine implements AudioEngine {
  JustAudioEngine({AudioPlayer? player}) : _player = player ?? AudioPlayer();

  final AudioPlayer _player;
  bool _sessionConfigured = false;

  @override
  Future<Duration?> load(Uri url, {Map<String, String> headers = const {}}) async {
    await _configureSession();
    // The bearer token rides on the request the platform player makes:
    // audio is a normal authenticated endpoint, not a public URL with a
    // signature in it.
    return _player.setAudioSource(AudioSource.uri(url, headers: headers));
  }

  /// Ask for the audio session once, the first time something is loaded.
  ///
  /// `music()` is what makes the clip behave the way a listener expects:
  /// it keeps playing with the screen off, it stops when headphones are
  /// pulled out, and it interrupts rather than mixing with whatever else
  /// was playing — which is right, because you are listening to this on
  /// purpose.
  Future<void> _configureSession() async {
    if (_sessionConfigured) return;
    final session = await AudioSession.instance;
    await session.configure(const AudioSessionConfiguration.music());
    _sessionConfigured = true;
  }

  @override
  Future<void> play() => _player.play();

  @override
  Future<void> pause() => _player.pause();

  @override
  Future<void> seek(Duration position) => _player.seek(position);

  @override
  Future<void> stop() => _player.stop();

  @override
  Stream<Duration> get positions => _player.positionStream;

  @override
  Stream<bool> get playing => _player.playingStream;

  @override
  Stream<Duration?> get durations => _player.durationStream;

  @override
  Stream<void> get completions => _player.processingStateStream
      .where((state) => state == ProcessingState.completed);

  @override
  Future<void> dispose() => _player.dispose();
}
