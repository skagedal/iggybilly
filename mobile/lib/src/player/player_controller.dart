import 'dart:async';

import 'package:flutter/foundation.dart';

import '../api/models.dart';
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
  PlayerController({required this._engine}) {
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
      _engine.completions.listen((_) {
        // Leave the clip loaded and wound back, so the obvious next
        // gesture — press play again — works.
        _isPlaying = false;
        _position = Duration.zero;
        notifyListeners();
      }),
    ]);
  }

  final AudioEngine _engine;
  final List<StreamSubscription<void>> _subscriptions = [];

  Clip? _clip;
  bool _isPlaying = false;
  Duration _position = Duration.zero;
  Duration? _duration;
  String? _error;

  /// What is loaded, or null when nothing is.
  Clip? get clip => _clip;
  bool get isPlaying => _isPlaying;
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

  /// Load a clip and start it — or, if it is already the loaded one,
  /// toggle it, so pressing play twice does not lose your place.
  Future<void> play(Clip clip, Uri url, {Map<String, String> headers = const {}}) async {
    if (_clip?.id == clip.id) {
      await toggle();
      return;
    }

    _clip = clip;
    _position = Duration.zero;
    _duration = null;
    _error = null;
    notifyListeners();

    try {
      final reported = await _engine.load(url, headers: headers);
      if (reported != null) _duration = reported;
      await _engine.play();
    } catch (e) {
      // A clip whose file is missing or in a format the device cannot
      // decode. Unload it: a bar showing a clip that will not play is
      // worse than no bar.
      _error = 'That clip could not be played.';
      _clip = null;
      _isPlaying = false;
    }
    notifyListeners();
  }

  /// Play or pause whatever is loaded.
  Future<void> toggle() async {
    if (_clip == null) return;
    if (_isPlaying) {
      await _engine.pause();
    } else {
      await _engine.play();
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

  /// Unload, used when the loaded clip is deleted.
  Future<void> stop() async {
    if (_clip == null) return;
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
}
