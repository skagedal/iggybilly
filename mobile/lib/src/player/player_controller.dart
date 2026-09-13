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
/// Every playback is a queue, and most queues hold one clip. Pressing
/// play in a label's playlist queues the whole label; the current clip is
/// found in the queue by id rather than held as an index, so a reorder
/// arriving under a playing queue needs no repair.
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

  /// The label the queue is the playlist of, or null for a lone clip.
  String? _source;
  List<Clip> _tracks = const [];

  /// Where to carry on once the playing clip has left [_tracks] — its
  /// label removed while it played: the first of its old successors
  /// still queued.
  int? _resumeAt;

  /// How to reach each queued clip's audio. Held rather than resolved
  /// up front, because only the clips actually reached are loaded.
  Uri Function(Clip clip) _urlFor = _noUrl;
  Map<String, String> _headers = const {};

  /// Clips in a row that would not load. A whole pass of them stops
  /// playback rather than spinning through a dead list.
  int _failures = 0;

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

  /// The label the queue plays from, or null when it is a lone clip.
  String? get queueSource => _source;

  /// Everything queued, the loaded clip included while it is still in it.
  List<Clip> get queue => _tracks;

  /// Where the loaded clip is in [queue], or -1 when it has left it.
  int get queueIndex => _tracks.indexWhere((c) => c.id == _clip?.id);

  /// Whether there is anything to go next or previous to.
  bool get hasQueue => _tracks.length > 1;

  bool get isPlaying => _isPlaying;

  /// Whether the queue starts again instead of ending.
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
    await _applyLoop();
  }

  /// Load a clip as a queue of one and start it — or, if it is already
  /// the loaded one, toggle it, so pressing play twice does not lose your
  /// place.
  Future<void> play(Clip clip, Uri url, {Map<String, String> headers = const {}}) async {
    if (_clip?.id == clip.id) {
      await toggle();
      return;
    }
    _source = null;
    _tracks = [clip];
    _urlFor = (_) => url;
    _headers = headers;
    await _load(clip);
  }

  /// Queue [clips] and start the one with [clipId]. [source] names the
  /// label the clips are the playlist of. Pressing play on the loaded
  /// clip toggles it, as [play] does, and adopts the queue.
  Future<void> playQueue(
    List<Clip> clips,
    int clipId, {
    String? source,
    required Uri Function(Clip clip) urlFor,
    Map<String, String> headers = const {},
  }) async {
    final target = clips.where((c) => c.id == clipId).firstOrNull;
    if (target == null) return;
    _source = source;
    _tracks = List.unmodifiable(clips);
    _resumeAt = null;
    _urlFor = urlFor;
    _headers = headers;
    if (_clip?.id == clipId) {
      notifyListeners();
      await _applyLoop();
      await toggle();
      return;
    }
    await _load(target);
  }

  /// Replace what is queued without touching what is playing — if the
  /// queue is the playlist of [source], and otherwise nothing. How a
  /// reorder, or a label going on or coming off, reaches the queue.
  Future<void> setQueueTracks(String source, List<Clip> clips) async {
    if (_source == null || _source!.toLowerCase() != source.toLowerCase()) {
      return;
    }
    final current = _clip?.id;
    final kept = {for (final c in clips) c.id};
    final index = queueIndex;
    if (current != null && index != -1 && !kept.contains(current)) {
      _resumeAt = _tracks
          .skip(index + 1)
          .where((c) => kept.contains(c.id))
          .firstOrNull
          ?.id;
    }
    _tracks = List.unmodifiable(clips);
    notifyListeners();
    await _applyLoop();
  }

  /// Play the clip after this one. On the last, wrap to the first when
  /// repeat is on, and otherwise stop wound back.
  Future<void> next() async {
    final current = _clip;
    if (current == null) return;
    final following = _following(current.id, repeat: _repeat);
    if (following == null) {
      await _engine.pause();
      await _engine.seek(Duration.zero);
      _position = Duration.zero;
      notifyListeners();
      return;
    }
    await _advanceTo(following);
  }

  /// Go back a clip — unless this one is past its opening seconds, in
  /// which case start it again, as every other player does.
  Future<void> previous() async {
    final index = queueIndex;
    if (_clip == null) return;
    if (_position > _restartAfter || index <= 0) {
      _position = Duration.zero;
      notifyListeners();
      await _engine.seek(Duration.zero);
      return;
    }
    await _load(_tracks[index - 1]);
  }

  /// Play a clip that is already queued.
  Future<void> jumpTo(int clipId) async {
    final target = _tracks.where((c) => c.id == clipId).firstOrNull;
    if (target == null) return;
    await _advanceTo(target);
  }

  /// Past this far into a clip, previous restarts it.
  static const _restartAfter = Duration(seconds: 3);

  static Uri _noUrl(Clip clip) =>
      throw StateError('nothing queued to resolve ${clip.id} for');

  /// Load a queued clip and start it.
  Future<void> _load(Clip clip) async {
    final url = _urlFor(clip);
    final headers = _headers;
    final generation = ++_generation;
    _clip = clip;
    _resumeAt = null;
    _position = Duration.zero;
    _duration = null;
    _error = null;
    notifyListeners();
    unawaited(_applyLoop());

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
      // decode — or one deleted after it was queued. In a queue, skip it,
      // unless every clip has now failed in turn.
      _failures++;
      final following = _following(clip.id, repeat: true);
      if (following != null &&
          following.id != clip.id &&
          _failures < _tracks.length) {
        await _load(following);
        return;
      }
      // Unload it: a bar showing a clip that will not play is worse than
      // no bar.
      _failures = 0;
      _error = 'That clip could not be played.';
      _clip = null;
      _isPlaying = false;
      notifyListeners();
      return;
    }
    _failures = 0;
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

  /// Repeat the queue, or stop repeating it. Remembered across
  /// launches: it is a way of listening, not a property of one clip.
  Future<void> setRepeat(bool value) async {
    if (value == _repeat) return;
    _repeat = value;
    notifyListeners();
    await _applyLoop();
    await _settings?.writeRepeat(value);
  }

  Future<void> toggleRepeat() => setRepeat(!_repeat);

  /// Unload, used when the loaded clip is deleted.
  Future<void> stop() async {
    if (_clip == null) return;
    // Any load still in flight is now for a clip nobody is listening to.
    _generation++;
    _clip = null;
    _source = null;
    _tracks = const [];
    _resumeAt = null;
    _isPlaying = false;
    _position = Duration.zero;
    _duration = null;
    notifyListeners();
    await _engine.stop();
  }

  /// Keep the bar's caption honest when the loaded clip is renamed.
  void renamed(int clipId, String name) {
    final current = _clip;
    _tracks = [
      for (final c in _tracks) c.id == clipId ? c.withName(name) : c,
    ];
    if (current == null || current.id != clipId) {
      notifyListeners();
      return;
    }
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

  /// Whether the platform loops the loaded clip itself.
  ///
  /// Only when repeat is on and the queue is exactly this clip. That
  /// keeps every loop that can be gapless gapless — a two-bar riff played
  /// twice with a hole in it is a different sound — and leaves a real
  /// playlist to wrap by advancing, where the seam is a change of clip
  /// anyway. It depends on the queue, so it is re-applied whenever the
  /// queue changes as well as when the toggle does.
  bool get _loopsCurrent =>
      _repeat && _tracks.length == 1 && _tracks.first.id == _clip?.id;

  Future<void> _applyLoop() => _engine.setLoopCurrent(_loopsCurrent);

  /// The clip to play after [current], or null when the queue has run out.
  Clip? _following(int current, {required bool repeat}) {
    if (_tracks.isEmpty) return null;
    final index = _tracks.indexWhere((c) => c.id == current);
    if (index == -1) {
      final resume = _tracks.where((c) => c.id == _resumeAt).firstOrNull;
      if (resume != null) return resume;
      return repeat ? _tracks.first : null;
    }
    if (index + 1 < _tracks.length) return _tracks[index + 1];
    return repeat ? _tracks.first : null;
  }

  Future<void> _advanceTo(Clip clip) async {
    if (clip.id == _clip?.id) {
      _position = Duration.zero;
      notifyListeners();
      await _engine.seek(Duration.zero);
      unawaited(_engine.play());
      return;
    }
    await _load(clip);
  }

  void _onCompleted() {
    _position = Duration.zero;
    final current = _clip;
    if (current == null) return;
    if (_loopsCurrent) {
      // The platform loops the clip itself, so reaching the end with
      // repeat on means it didn't. Starting it again here is a seam
      // between the loops, which is still better than repeat quietly
      // stopping.
      notifyListeners();
      unawaited(_engine.seek(Duration.zero).then((_) => _engine.play()));
      return;
    }
    final following = _following(current.id, repeat: _repeat);
    if (following != null) {
      unawaited(_advanceTo(following));
      return;
    }
    // Leave the clip loaded and wound back, so the obvious next
    // gesture — press play again — works.
    _isPlaying = false;
    notifyListeners();
  }
}
