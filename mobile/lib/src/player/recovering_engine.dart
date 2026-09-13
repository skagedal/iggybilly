import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:just_audio/just_audio.dart' show PlayerInterruptedException;

import 'audio_engine.dart';

/// An engine that throws the platform player away when it stops working.
///
/// This exists because of one symptom: you press play, the bar appears
/// for an instant and vanishes, and every clip you try after that does
/// the same until the app is force-quit. The reason is that a platform
/// player which fails to load a source can stay unusable — every later
/// `setAudioSource` on it fails too — and the app held exactly one of
/// them for its whole life. Force-quitting worked because it was the only
/// thing that built a new one.
///
/// So: a failed load gets one more try on a brand new player. A clip that
/// really is broken still fails, and says so, but it no longer takes the
/// rest of the session with it.
///
/// The delegate arrives as a factory rather than an instance, which is
/// what makes this testable: the platform is one [AudioEngine] away, so
/// the replace-and-retry rule can be exercised against a fake that fails
/// on demand.
class RecoveringAudioEngine implements AudioEngine {
  RecoveringAudioEngine(this._create);

  final AudioEngine Function() _create;

  /// Broadcast, and owned here rather than forwarded, because the whole
  /// point is that the thing behind them is replaced. A listener that
  /// had subscribed to the delegate's streams would go deaf the moment
  /// one was swapped out, which is the bug this class is about.
  final _positions = StreamController<Duration>.broadcast();
  final _playing = StreamController<bool>.broadcast();
  final _durations = StreamController<Duration?>.broadcast();
  final _completions = StreamController<void>.broadcast();

  AudioEngine? _delegate;
  final List<StreamSubscription<void>> _piped = [];

  /// Re-applied to every replacement: loop mode belongs to the player,
  /// and a new player has forgotten it.
  bool _loop = false;
  bool _disposed = false;

  @override
  Future<Duration?> load(Uri url, {Map<String, String> headers = const {}}) async {
    try {
      return await _attached().load(url, headers: headers);
    } on PlayerInterruptedException {
      // Not a broken player: a newer load superseded this one. Replacing
      // it here would destroy the load that is now in charge.
      rethrow;
    } catch (error) {
      if (_disposed) rethrow;
      debugPrint('iggybilly: load failed ($error); replacing the player');
      await _replace();
      // One retry, on a player with no history. If this fails too, the
      // clip is genuinely unplayable — but the next clip will still work,
      // which is the difference that matters.
      return _attached().load(url, headers: headers);
    }
  }

  @override
  Future<void> play() async => _delegate?.play();

  @override
  Future<void> pause() async => _delegate?.pause();

  @override
  Future<void> seek(Duration position) async => _delegate?.seek(position);

  @override
  Future<void> stop() async => _delegate?.stop();

  @override
  Future<void> setLoopCurrent(bool loop) async {
    _loop = loop;
    await _delegate?.setLoopCurrent(loop);
  }

  @override
  Stream<Duration> get positions => _positions.stream;

  @override
  Stream<bool> get playing => _playing.stream;

  @override
  Stream<Duration?> get durations => _durations.stream;

  @override
  Stream<void> get completions => _completions.stream;

  @override
  Future<void> dispose() async {
    _disposed = true;
    await _detach();
    await _positions.close();
    await _playing.close();
    await _durations.close();
    await _completions.close();
  }

  /// The current delegate, built and wired up if there isn't one.
  ///
  /// Built lazily rather than in the constructor so that a player is
  /// only asked of the platform once something is actually played, which
  /// is also what keeps the audio session out of the way until then.
  AudioEngine _attached() {
    final existing = _delegate;
    if (existing != null) return existing;

    final delegate = _create();
    _delegate = delegate;
    _piped.addAll([
      delegate.positions.listen(_positions.add),
      delegate.playing.listen(_playing.add),
      delegate.durations.listen(_durations.add),
      delegate.completions.listen(_completions.add),
    ]);
    if (_loop) unawaited(delegate.setLoopCurrent(true));
    return delegate;
  }

  Future<void> _replace() async {
    await _detach();
    // The listener saw the old player stop, and nothing is loaded on the
    // new one. Saying so keeps a bar from showing a pause button for a
    // player that no longer exists.
    if (!_playing.isClosed) _playing.add(false);
  }

  Future<void> _detach() async {
    for (final subscription in _piped) {
      await subscription.cancel();
    }
    _piped.clear();

    final delegate = _delegate;
    _delegate = null;
    if (delegate == null) return;
    try {
      await delegate.dispose();
    } catch (error) {
      // A player that is already wedged may well fail to shut down
      // cleanly. It is on its way out either way.
      debugPrint('iggybilly: discarded player did not dispose ($error)');
    }
  }
}
