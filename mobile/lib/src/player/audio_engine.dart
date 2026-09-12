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

  /// Loop the loaded clip rather than ending it.
  ///
  /// The platform's own looping, not a seek on completion: on Android,
  /// iOS and macOS it is gapless, and a bar-length riff played twice with
  /// a hole in the middle is not the same thing as a bar-length riff
  /// played twice. Not the repeat toggle itself: a queue of several clips
  /// repeats by advancing, and [PlayerController] decides which applies.
  Future<void> setLoopCurrent(bool loop);

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
  final List<StreamSubscription<void>> _sessionSubscriptions = [];
  bool _sessionConfigured = false;

  /// Whether the clip was playing when something interrupted it, and so
  /// whether to start it again when the interruption ends.
  bool _resumeAfterInterruption = false;

  @override
  Future<Duration?> load(Uri url, {Map<String, String> headers = const {}}) async {
    await _configureSession();
    // The bearer token rides on the request the platform player makes:
    // audio is a normal authenticated endpoint, not a public URL with a
    // signature in it. A local file needs no headers and is given none.
    return _player.setAudioSource(
      AudioSource.uri(url, headers: headers.isEmpty ? null : headers),
    );
  }

  /// Ask for the audio session once, the first time something is loaded.
  ///
  /// `music()` is what makes the clip behave the way a listener expects:
  /// it keeps playing with the screen off, and it interrupts rather than
  /// mixing with whatever else was playing — which is right, because you
  /// are listening to this on purpose.
  ///
  /// Configuring the session is only half of it. The other half is
  /// answering what the session then tells us, which is what the two
  /// subscriptions below are for; see [_onInterruption].
  Future<void> _configureSession() async {
    if (_sessionConfigured) return;
    final session = await AudioSession.instance;
    await session.configure(const AudioSessionConfiguration.music());
    _sessionSubscriptions.addAll([
      session.interruptionEventStream.listen(_onInterruption),
      // Headphones pulled out, or a Bluetooth speaker walked away from.
      // Nothing pauses for this on its own: without this line the clip
      // carries on out of the phone's own speaker, in a meeting.
      session.becomingNoisyEventStream.listen((_) {
        _resumeAfterInterruption = false;
        unawaited(_player.pause());
      }),
    ]);
    _sessionConfigured = true;
  }

  /// Something else wanted the audio output, and now it doesn't.
  ///
  /// This is the difference between a clip that stops in your pocket and
  /// one that doesn't. A call, an alarm, a navigation instruction or
  /// another app taking the session pauses the player at the platform
  /// level; nothing starts it again unless we do. Without this, "it was
  /// playing and then it just stopped" is the expected behaviour, and
  /// the cause is invisible by the time you look at the phone.
  void _onInterruption(AudioInterruptionEvent event) {
    if (event.begin) {
      switch (event.type) {
        case AudioInterruptionType.duck:
          // A navigation instruction over the top: quieter, not stopped.
          unawaited(_player.setVolume(0.3));
        case AudioInterruptionType.pause:
        case AudioInterruptionType.unknown:
          _resumeAfterInterruption = _player.playing;
          unawaited(_player.pause());
      }
      return;
    }

    switch (event.type) {
      case AudioInterruptionType.duck:
        unawaited(_player.setVolume(1));
      case AudioInterruptionType.pause:
        // The platform said this interruption was resumable, so carry on
        // if we were the one playing when it started.
        if (_resumeAfterInterruption) unawaited(_player.play());
        _resumeAfterInterruption = false;
      case AudioInterruptionType.unknown:
        // Possibly indefinite — another app may still hold the session.
        // Starting again here is how two apps end up playing at once.
        _resumeAfterInterruption = false;
    }
  }

  @override
  Future<void> play() => _player.play();

  @override
  Future<void> pause() {
    // A deliberate pause outranks a pending resume: the user pressing
    // pause during a phone call means pause, not "start again when the
    // call ends".
    _resumeAfterInterruption = false;
    return _player.pause();
  }

  @override
  Future<void> seek(Duration position) => _player.seek(position);

  @override
  Future<void> stop() {
    _resumeAfterInterruption = false;
    return _player.stop();
  }

  @override
  Future<void> setLoopCurrent(bool loop) =>
      _player.setLoopMode(loop ? LoopMode.one : LoopMode.off);

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
  Future<void> dispose() async {
    for (final subscription in _sessionSubscriptions) {
      await subscription.cancel();
    }
    _sessionSubscriptions.clear();
    await _player.dispose();
  }
}
