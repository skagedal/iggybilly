import 'dart:async';

import 'package:flutter/foundation.dart';

import '../api/models.dart';
import '../cache/track_cache.dart';
import '../settings.dart';
import 'audio_engine.dart';

/// The one player in the app.
///
/// There is a single instance, owned above the navigator, so moving
/// between screens never interrupts what is playing — the same reason
/// the web version keeps its player in a bar outside the router outlet.
/// Playing a second clip takes the player over.
///
/// The platform arrives as an [AudioEngine], so every rule below can be
/// tested: pressing play on the loaded clip toggles rather than
/// restarting it, a clip the server could not decode still reports a
/// duration once the platform finds one, and seeking before anything is
/// loaded does nothing rather than throwing.
class PlayerController extends ChangeNotifier {
  PlayerController({
    required this._engine,
    this._cache,
    this._settings,
  }) {
    _subscriptions.addAll([
      _engine.positions.listen((p) {
        _position = p;
        notifyListeners();
      }),
      _engine.playing.listen((p) {
        _isPlaying = p;
        notifyListeners();
      }),
      _engine.durations.listen((d) {
        if (d != null) {
          _duration = d;
          notifyListeners();
        }
      }),
      _engine.completions.listen((_) => _onCompleted()),
    ]);
  }

  final AudioEngine _engine;

  /// Null in a test that only cares about playback, and in that case
  /// every clip streams — which is what the app did before there was a
  /// cache.
  final TrackCache? _cache;
  final Settings? _settings;

  final List<StreamSubscription<void>> _subscriptions = [];

  Clip? _clip;
  bool _isPlaying = false;
  bool _repeat = false;
  Duration _position = Duration.zero;
  Duration? _duration;
  String? _error;

  /// Which call to [play] the player is currently answering to.
  ///
  /// Loading is asynchronous and can fail after the fact, so a load that
  /// has been superseded must not be allowed to write state for a clip
  /// that is no longer the one on screen — which is how pressing play on
  /// a second clip used to make the bar appear and vanish.
  int _generation = 0;

  /// What is loaded, or null when nothing is.
  Clip? get clip => _clip;

  bool get isPlaying => _isPlaying;

  /// Whether the loaded clip starts again instead of ending.
  bool get repeat => _repeat;

  Duration get position => _position;

  /// The clip's length: what the platform reports once it knows, else
  /// what the server measured at upload. Null when neither knows yet.
  Duration? get duration => _duration ?? _clip?.duration;

  /// Why the last clip would not play, if it wouldn't.
  String? get error => _error;

  /// How far through, 0 to 1. Zero rather than a division by zero while
  /// the length is still unknown.
  double get progress {
    final total = duration;
    if (total == null || total.inMicroseconds <= 0) return 0;
    final fraction = _position.inMicroseconds / total.inMicroseconds;
    return fraction.clamp(0.0, 1.0);
  }

  /// Pick up the settings that outlive a launch.
  Future<void> restore() async {
    final settings = _settings;
    if (settings == null) return;
    final repeat = await settings.readRepeat();
    if (repeat == _repeat) return;
    _repeat = repeat;
    notifyListeners();
    await _engine.setRepeat(repeat);
  }

  /// Load a clip and start it — or, if it is already the loaded one,
  /// toggle it, so pressing play twice does not lose your place.
  Future<void> play(Clip clip, Uri url, {Map<String, String> headers = const {}}) async {
    if (_clip?.id == clip.id) {
      await toggle();
      return;
    }

    final generation = ++_generation;
    _clip = clip;
    _position = Duration.zero;
    _duration = null;
    _error = null;
    notifyListeners();

    final cache = _cache;
    // A clip that is already on disk plays from there. That is the
    // difference between a phone in a pocket with one bar of signal and
    // one on the desk: no request to stall, and nothing for the OS to
    // refuse while the app is in the background.
    final local = cache?.fileFor(url);

    var started = false;
    if (local != null) {
      started = await _start(generation, local.uri, const {});
      if (generation != _generation) return;
      if (!started) {
        // The local copy will not play: truncated, or in a container
        // this device turns out not to read. Forget it and go to the
        // server, so a bad file costs one slow play rather than a clip
        // that never works again.
        await cache!.forget(url);
        if (generation != _generation) return;
      }
    }
    if (!started) {
      started = await _start(generation, url, headers);
      if (generation != _generation) return;
    }

    if (!started) {
      // A clip whose file is missing, or in a format the device cannot
      // decode. Unload it: a bar showing a clip that will not play is
      // worse than no bar.
      _error = 'That clip could not be played.';
      _clip = null;
      _isPlaying = false;
      notifyListeners();
      return;
    }
    notifyListeners();

    if (cache == null) return;
    if (cache.hasCopy(url)) {
      // Played now, so last in line to be evicted.
      await cache.touch(clip, url);
    } else {
      // Fetched for next time rather than waited for now: making every
      // first play wait for a download would be a worse app than one
      // that streams once.
      unawaited(cache.store(clip, url, headers: headers));
    }
  }

  /// Play or pause whatever is loaded.
  Future<void> toggle() async {
    if (_clip == null) return;
    if (_isPlaying) {
      await _engine.pause();
    } else {
      unawaited(_engine.play());
    }
  }

  /// Seek to a fraction of the clip, as a drag on the waveform gives.
  /// Ignored when nothing is loaded or the length is not known yet —
  /// there is nothing to be a fraction of.
  Future<void> seekFraction(double fraction) async {
    final total = duration;
    if (_clip == null || total == null) return;
    final clamped = fraction.clamp(0.0, 1.0);
    final target = Duration(
      microseconds: (total.inMicroseconds * clamped).round(),
    );
    _position = target;
    notifyListeners();
    await _engine.seek(target);
  }

  /// Jump by [offset], clamped to the clip. What the skip buttons in the
  /// player pane do; a negative offset goes back.
  Future<void> skip(Duration offset) async {
    final total = duration;
    if (_clip == null) return;
    var target = _position + offset;
    if (target.isNegative) target = Duration.zero;
    if (total != null && target > total) target = total;
    _position = target;
    notifyListeners();
    await _engine.seek(target);
  }

  /// Loop the loaded clip, or stop looping it. Remembered across
  /// launches: it is a way of listening, not a property of one clip.
  Future<void> setRepeat(bool value) async {
    if (value == _repeat) return;
    _repeat = value;
    notifyListeners();
    await _engine.setRepeat(value);
    await _settings?.writeRepeat(value);
  }

  Future<void> toggleRepeat() => setRepeat(!_repeat);

  /// Unload, used when the loaded clip is deleted.
  Future<void> stop() async {
    if (_clip == null) return;
    // Any load still in flight is now for a clip nobody is listening to.
    _generation++;
    _clip = null;
    _isPlaying = false;
    _position = Duration.zero;
    _duration = null;
    notifyListeners();
    await _engine.stop();
  }

  /// Keep the bar's caption honest when the loaded clip is renamed.
  void renamed(int clipId, String name) {
    final current = _clip;
    if (current == null || current.id != clipId) return;
    _clip = current.withName(name);
    notifyListeners();
  }

  /// Forget a play failure once it has been shown.
  void clearError() {
    if (_error == null) return;
    _error = null;
    notifyListeners();
  }

  @override
  void dispose() {
    for (final s in _subscriptions) {
      s.cancel();
    }
    _engine.dispose();
    super.dispose();
  }

  /// Hand a source to the engine and start it. False when the engine
  /// would not take it.
  Future<bool> _start(
    int generation,
    Uri source,
    Map<String, String> headers,
  ) async {
    try {
      final reported = await _engine.load(source, headers: headers);
      if (generation != _generation) return true;
      if (reported != null) _duration = reported;
      // Not awaited: play() completes when playback *finishes*, not when
      // it starts. Awaiting it would park the rest of this method until
      // the clip ends.
      unawaited(_engine.play());
      return true;
    } catch (_) {
      return false;
    }
  }

  void _onCompleted() {
    _position = Duration.zero;
    if (_repeat && _clip != null) {
      // The platform loops the clip itself, so reaching the end with
      // repeat on means it didn't. Starting it again here is a seam
      // between the loops, which is still better than repeat quietly
      // stopping.
      notifyListeners();
      unawaited(_engine.seek(Duration.zero).then((_) => _engine.play()));
      return;
    }
    // Leave the clip loaded and wound back, so the obvious next
    // gesture — press play again — works.
    _isPlaying = false;
    notifyListeners();
  }
}
