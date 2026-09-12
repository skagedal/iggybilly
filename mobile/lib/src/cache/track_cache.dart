import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;

import '../api/models.dart';

/// The clips the app has on disk.
///
/// Two things live in here, and they are deliberately different. The
/// cache proper is automatic and bounded: playing a clip copies it here,
/// and once the total passes [maxBytes] the clip nobody has played for
/// longest is deleted to make room. A *kept* clip is a promise instead —
/// the user asked for it by name, so it stays until they say otherwise
/// and it does not count towards [maxBytes]. A budget for "how much may
/// this app spend on guessing what I'll want" should not be spent on the
/// files that were asked for.
///
/// Everything is best-effort. A cache that cannot be opened, a download
/// that fails, a phone that is full: each of those makes the next play
/// stream from the server, which is what the app did before this class
/// existed. None of them is allowed to stop a clip playing.
class TrackCache extends ChangeNotifier {
  TrackCache({
    required this._locate,
    http.Client? httpClient,
  })  : _http = httpClient ?? http.Client(),
        _ownsHttp = httpClient == null;

  /// The default ceiling, 500 MB. The server caps a clip at 10 MB, so
  /// this is a few hundred of them — more than a band's working set, and
  /// small enough that nobody notices it in the phone's storage screen.
  static const defaultMaxBytes = 500 * 1024 * 1024;

  /// The ceilings the storage screen offers. A free-text byte count
  /// would be a worse control than five sensible numbers.
  static const sizeChoices = <int>[
    100 * 1024 * 1024,
    250 * 1024 * 1024,
    defaultMaxBytes,
    1024 * 1024 * 1024,
    2 * 1024 * 1024 * 1024,
  ];

  /// Where the index lives, inside the cache directory. Named so that
  /// [_reconcile] can tell it from a clip.
  static const _indexName = 'index.json';

  /// A download in progress. Renamed onto its real name only once it is
  /// complete, so a file in the directory is always a whole clip.
  static const _partSuffix = '.part';

  final Future<Directory> Function() _locate;
  final http.Client _http;
  final bool _ownsHttp;

  /// Null until [open] has found the directory, and again if it could
  /// not: the cache is then a no-op rather than a failure.
  Directory? _directory;

  final Map<String, _Entry> _entries = {};
  final Map<String, Future<File?>> _inFlight = {};

  int _maxBytes = defaultMaxBytes;
  Future<void>? _opening;
  bool _disposed = false;

  /// Find the directory, read the index and reconcile it with what is
  /// actually on disk. Safe to call more than once; the work happens
  /// once.
  Future<void> open() => _opening ??= _open();

  /// Whether the cache found somewhere to write. False makes every read
  /// a miss and every write a no-op.
  bool get isUsable => _directory != null;

  /// The ceiling on the automatic cache. Kept clips are not counted
  /// against it.
  int get maxBytes => _maxBytes;

  /// What the automatic cache is using, which is what [maxBytes] bounds.
  int get usedBytes => _sum(kept: false);

  /// What the clips the user asked to keep are using. Shown next to
  /// [usedBytes] rather than added to it, so the two numbers explain
  /// themselves.
  int get keptBytes => _sum(kept: true);

  /// Every clip on disk that the user asked to keep, most recently
  /// played first — what the storage screen lists.
  List<CachedTrack> get keptTracks {
    final kept = _entries.values.where((e) => e.kept).toList()
      ..sort((a, b) => b.playedAt.compareTo(a.playedAt));
    return kept.map(_track).toList(growable: false);
  }

  /// How many clips the automatic cache is holding.
  int get cachedCount => _entries.values.where((e) => !e.kept).length;

  /// The identity of a clip's audio, as far as the cache is concerned.
  ///
  /// The absolute URL with the query dropped: clip ids are only unique
  /// within one server, so the server has to be part of the key, and
  /// `?download=1` names the same bytes as the bare path.
  static String keyFor(Uri url) => Uri(
        scheme: url.scheme,
        host: url.host,
        port: url.hasPort ? url.port : null,
        path: url.path,
      ).toString();

  /// The local copy of [url]'s audio, or null when there isn't one.
  ///
  /// Synchronous on purpose. [PlayerController.play] has to choose
  /// between a file and a stream before it puts anything on screen, and
  /// an await there is a frame in which the bar has nothing to say.
  File? fileFor(Uri url) {
    final directory = _directory;
    final entry = _entries[keyFor(url)];
    if (directory == null || entry == null) return null;
    return File(p.join(directory.path, entry.file));
  }

  /// Whether [url] is on disk at all, cached or kept.
  bool hasCopy(Uri url) => _entries.containsKey(keyFor(url));

  /// Whether the user asked to keep [url] rather than let it be evicted.
  bool isKept(Uri url) => _entries[keyFor(url)]?.kept ?? false;

  /// Whether [url] is being fetched right now. What a switch that has
  /// been flipped but has nothing to show for it yet reads.
  bool isDownloading(Uri url) => _inFlight.containsKey(keyFor(url));

  /// Put a clip's audio on disk, unless it is already there.
  ///
  /// Returns the file, or null when the cache is unusable or the
  /// download failed. Callers treat null as "stream it": nothing is
  /// broken by a miss beyond the next play not being local.
  Future<File?> store(
    Clip clip,
    Uri url, {
    Map<String, String> headers = const {},
    bool kept = false,
  }) {
    final key = keyFor(url);
    // One download per clip however many callers ask for it: pressing
    // play both starts the clip and asks for it to be cached.
    final existing = _inFlight[key];
    if (existing != null) return existing;

    final download = _download(clip, url, key, headers: headers, kept: kept);
    _inFlight[key] = download;
    // Told twice, so that a switch flipped on a slow connection can say
    // it is working rather than sitting there doing nothing visible.
    _notify();
    return download.whenComplete(() {
      _inFlight.remove(key);
      _notify();
    });
  }

  /// Record that a clip has just been played, which is what the
  /// eviction order is built from.
  Future<void> touch(Clip clip, Uri url) async {
    final key = keyFor(url);
    final entry = _entries[key];
    if (entry == null) return;
    final renamed = entry.name != clip.name;
    _entries[key] = entry.playedNow(clip.name);
    await _writeIndex();
    // Only a rename is worth a rebuild; nothing on screen shows when a
    // clip was last played.
    if (renamed) _notify();
  }

  /// Keep a clip on disk, or stop keeping it.
  ///
  /// Turning this on downloads the clip if it isn't here yet — the whole
  /// point of the switch is that the clip will play with no network.
  /// Turning it off leaves the file in the automatic cache, where it
  /// takes its chances with everything else.
  ///
  /// Returns whether the clip is now kept. False means the download did
  /// not work, which the caller should say out loud: a switch that
  /// quietly springs back is the worst way to learn there was no network.
  Future<bool> setKept(
    Clip clip,
    Uri url,
    bool kept, {
    Map<String, String> headers = const {},
  }) async {
    final key = keyFor(url);
    if (!kept) {
      await _updateKept(key, false);
      return false;
    }
    if (_entries.containsKey(key)) {
      await _updateKept(key, true);
      return true;
    }
    return await store(clip, url, headers: headers, kept: true) != null;
  }

  /// Stop keeping a clip that [keptTracks] listed.
  ///
  /// It drops into the automatic cache rather than being deleted, where
  /// it takes its chances with everything else.
  Future<void> stopKeeping(CachedTrack track) => _updateKept(track.key, false);

  /// Drop a clip from disk, kept or not.
  ///
  /// For a local copy that turns out not to play: the player calls this
  /// and goes back to the server, rather than leaving a file that will
  /// fail the same way every time.
  Future<void> forget(Uri url) async {
    final entry = _entries[keyFor(url)];
    if (entry == null) return;
    await _delete(entry);
    await _writeIndex();
    _notify();
  }

  /// Change the ceiling, evicting down to it straight away.
  Future<void> setMaxBytes(int bytes) async {
    if (bytes <= 0 || bytes == _maxBytes) return;
    _maxBytes = bytes;
    await _evict();
    await _writeIndex();
    _notify();
  }

  /// Delete everything the user did not ask to keep.
  Future<void> clearCache() async {
    for (final entry in _entries.values.where((e) => !e.kept).toList()) {
      await _delete(entry);
    }
    await _writeIndex();
    _notify();
  }

  @override
  void dispose() {
    _disposed = true;
    if (_ownsHttp) _http.close();
    super.dispose();
  }

  /// Tell listeners, unless the cache has been disposed.
  ///
  /// A download that finishes after the app has torn the cache down would
  /// otherwise notify a disposed notifier, which throws.
  void _notify() {
    if (_disposed) return;
    notifyListeners();
  }

  // -- Opening ------------------------------------------------------------

  Future<void> _open() async {
    try {
      final directory = await _locate();
      await directory.create(recursive: true);
      _directory = directory;
      await _readIndex(directory);
      await _reconcile(directory);
      await _evict();
      await _writeIndex();
    } catch (e) {
      // No writable directory, or one we cannot read. Every lookup
      // misses from here on, which is the behaviour the app had before
      // there was a cache at all.
      debugPrint('iggybilly: track cache unavailable ($e)');
      _directory = null;
      _entries.clear();
    }
    _notify();
  }

  Future<void> _readIndex(Directory directory) async {
    final file = File(p.join(directory.path, _indexName));
    if (!await file.exists()) return;
    try {
      final raw = jsonDecode(await file.readAsString());
      if (raw is! Map<String, dynamic>) return;
      final max = raw['maxBytes'];
      if (max is int && max > 0) _maxBytes = max;
      for (final item in (raw['entries'] as List<dynamic>? ?? const [])) {
        if (item is! Map<String, dynamic>) continue;
        final entry = _Entry.fromJson(item);
        if (entry != null) _entries[entry.key] = entry;
      }
    } catch (e) {
      // A half-written index from a process that died mid-write. The
      // files are still there and [_reconcile] will clear them out; the
      // cost is one cold start, not a broken app.
      debugPrint('iggybilly: unreadable cache index, starting empty ($e)');
      _entries.clear();
    }
  }

  /// Make the index and the directory agree.
  ///
  /// Both drift. The OS may clear an app's caches under storage
  /// pressure without telling anyone, and a download interrupted by a
  /// crash leaves a `.part` nothing points at.
  Future<void> _reconcile(Directory directory) async {
    for (final entry in _entries.values.toList()) {
      final file = File(p.join(directory.path, entry.file));
      if (!await file.exists()) _entries.remove(entry.key);
    }

    final live = {for (final entry in _entries.values) entry.file};
    await for (final item in directory.list(followLinks: false)) {
      if (item is! File) continue;
      final name = p.basename(item.path);
      if (name == _indexName || live.contains(name)) continue;
      try {
        await item.delete();
      } catch (_) {
        // Someone else's file, or a permission we don't have. Leaving it
        // costs disk; failing here would cost the cache.
      }
    }
  }

  // -- Downloading --------------------------------------------------------

  Future<File?> _download(
    Clip clip,
    Uri url,
    String key, {
    required Map<String, String> headers,
    required bool kept,
  }) async {
    await open();
    final directory = _directory;
    if (directory == null) return null;

    final existing = _entries[key];
    if (existing != null) {
      if (kept && !existing.kept) await _updateKept(key, true);
      return File(p.join(directory.path, existing.file));
    }

    final name = _uniqueName(clip, url);
    final target = File(p.join(directory.path, name));
    final partial = File('${target.path}$_partSuffix');
    var written = 0;

    try {
      final request = http.Request('GET', url)..headers.addAll(headers);
      final response = await _http.send(request);
      if (response.statusCode != 200) {
        throw http.ClientException(
          'The server returned ${response.statusCode}.',
          url,
        );
      }

      final sink = partial.openWrite();
      try {
        await response.stream.forEach((chunk) {
          written += chunk.length;
          sink.add(chunk);
        });
      } finally {
        await sink.close();
      }

      final expected = response.contentLength;
      if (expected != null && expected != written) {
        throw http.ClientException(
          'Expected $expected bytes but got $written.',
          url,
        );
      }
      if (written == 0) throw http.ClientException('Empty response.', url);

      await partial.rename(target.path);
    } catch (e) {
      // A truncated file is worse than no file: it would play as a clip
      // that stops early and read as a corrupt upload.
      try {
        if (await partial.exists()) await partial.delete();
      } catch (_) {}
      debugPrint('iggybilly: could not cache “${clip.name}” ($e)');
      return null;
    }

    _entries[key] = _Entry(
      key: key,
      clipId: clip.id,
      name: clip.name,
      file: name,
      bytes: written,
      playedAt: DateTime.now().toUtc(),
      kept: kept,
    );
    // Protected: evicting what we just fetched to play would be absurd,
    // even when it alone is over the ceiling.
    await _evict(protect: key);
    await _writeIndex();
    _notify();
    return target;
  }

  /// A filename for a clip's audio, unique within the directory.
  ///
  /// Readable rather than hashed, because the first thing anyone does
  /// with a cache that misbehaves is list the directory. The index, not
  /// this, is what maps a key to its file.
  String _uniqueName(Clip clip, Uri url) {
    final host = url.host.replaceAll(RegExp(r'[^A-Za-z0-9.\-]'), '_');
    final stem = '${clip.id}-$host-${url.port}';
    final extension = _extensionFor(clip);
    final taken = {for (final entry in _entries.values) entry.file};

    var candidate = '$stem$extension';
    var suffix = 2;
    while (taken.contains(candidate)) {
      candidate = '$stem-$suffix$extension';
      suffix++;
    }
    return candidate;
  }

  /// The extension to give the local copy.
  ///
  /// It matters: AVPlayer takes the extension of a local file as its
  /// first hint at the container, and gets less forgiving the less it
  /// has to go on. The server's content type is the authority — it is
  /// chosen from an allow-list at upload — with the uploaded filename as
  /// a fallback for clips stored before that was recorded.
  static String _extensionFor(Clip clip) {
    const byContentType = {
      'audio/mpeg': '.mp3',
      'audio/mp4': '.m4a',
      'audio/wav': '.wav',
      'audio/x-wav': '.wav',
      'audio/flac': '.flac',
      'audio/ogg': '.ogg',
      'audio/aac': '.aac',
      'audio/webm': '.webm',
    };
    final known = byContentType[clip.contentType.toLowerCase()];
    if (known != null) return known;

    final extension = p.extension(clip.originalFilename).toLowerCase();
    if (RegExp(r'^\.[a-z0-9]{1,5}$').hasMatch(extension)) return extension;
    return '.audio';
  }

  // -- Bookkeeping --------------------------------------------------------

  Future<void> _updateKept(String key, bool kept) async {
    final entry = _entries[key];
    if (entry == null || entry.kept == kept) return;
    _entries[key] = entry.withKept(kept);
    // No longer kept means it now counts towards the ceiling, which it
    // may already have pushed past.
    if (!kept) await _evict();
    await _writeIndex();
    _notify();
  }

  /// Delete least-recently-played clips until the automatic cache fits.
  ///
  /// [protect] is spared whatever its age: it is the clip that is
  /// playing, or about to.
  Future<void> _evict({String? protect}) async {
    if (_directory == null) return;
    var used = usedBytes;
    if (used <= _maxBytes) return;

    final candidates = _entries.values
        .where((entry) => !entry.kept && entry.key != protect)
        .toList()
      ..sort((a, b) => a.playedAt.compareTo(b.playedAt));

    for (final entry in candidates) {
      if (used <= _maxBytes) break;
      await _delete(entry);
      used -= entry.bytes;
    }
  }

  Future<void> _delete(_Entry entry) async {
    final directory = _directory;
    _entries.remove(entry.key);
    if (directory == null) return;
    try {
      final file = File(p.join(directory.path, entry.file));
      if (await file.exists()) await file.delete();
    } catch (e) {
      // The entry is gone either way; a file we cannot delete is
      // wasted disk, not a wrong answer.
      debugPrint('iggybilly: could not delete ${entry.file} ($e)');
    }
  }

  Future<void> _writeIndex() async {
    final directory = _directory;
    if (directory == null) return;
    final body = jsonEncode({
      'version': 1,
      'maxBytes': _maxBytes,
      'entries': [for (final entry in _entries.values) entry.toJson()],
    });
    try {
      // Written beside the real file and renamed over it, so a process
      // killed mid-write leaves the old index rather than half of a new
      // one.
      final temporary = File(p.join(directory.path, '$_indexName$_partSuffix'));
      await temporary.writeAsString(body, flush: true);
      await temporary.rename(p.join(directory.path, _indexName));
    } catch (e) {
      debugPrint('iggybilly: could not write the cache index ($e)');
    }
  }

  int _sum({required bool kept}) => _entries.values
      .where((entry) => entry.kept == kept)
      .fold(0, (total, entry) => total + entry.bytes);

  CachedTrack _track(_Entry entry) => CachedTrack(
        key: entry.key,
        clipId: entry.clipId,
        name: entry.name,
        bytes: entry.bytes,
        lastPlayedAt: entry.playedAt.toLocal(),
        isKept: entry.kept,
      );
}

/// One clip on disk, as the storage screen shows it.
class CachedTrack {
  const CachedTrack({
    required this.key,
    required this.clipId,
    required this.name,
    required this.bytes,
    required this.lastPlayedAt,
    required this.isKept,
  });

  /// The cache's own name for this clip's audio. Opaque: it exists so
  /// that a row on screen can be acted on without rebuilding the URL it
  /// came from, and because a clip id alone is ambiguous across servers.
  final String key;

  final int clipId;
  final String name;
  final int bytes;
  final DateTime lastPlayedAt;
  final bool isKept;
}

/// One row of the index.
class _Entry {
  const _Entry({
    required this.key,
    required this.clipId,
    required this.name,
    required this.file,
    required this.bytes,
    required this.playedAt,
    required this.kept,
  });

  /// Returns null for a row that cannot be read, which is dropped rather
  /// than allowed to take the whole index down with it.
  static _Entry? fromJson(Map<String, dynamic> json) {
    final key = json['key'];
    final file = json['file'];
    final bytes = json['bytes'];
    final playedAt = DateTime.tryParse(json['playedAt'] as String? ?? '');
    if (key is! String || file is! String || bytes is! int || playedAt == null) {
      return null;
    }
    return _Entry(
      key: key,
      clipId: json['clipId'] as int? ?? 0,
      name: json['name'] as String? ?? '',
      file: file,
      bytes: bytes,
      playedAt: playedAt,
      kept: json['kept'] as bool? ?? false,
    );
  }

  final String key;
  final int clipId;
  final String name;
  final String file;
  final int bytes;

  /// UTC. The index outlives a time zone change.
  final DateTime playedAt;
  final bool kept;

  _Entry playedNow(String currentName) => _Entry(
        key: key,
        clipId: clipId,
        name: currentName,
        file: file,
        bytes: bytes,
        playedAt: DateTime.now().toUtc(),
        kept: kept,
      );

  _Entry withKept(bool value) => _Entry(
        key: key,
        clipId: clipId,
        name: name,
        file: file,
        bytes: bytes,
        playedAt: playedAt,
        kept: value,
      );

  Map<String, dynamic> toJson() => {
        'key': key,
        'clipId': clipId,
        'name': name,
        'file': file,
        'bytes': bytes,
        'playedAt': playedAt.toIso8601String(),
        'kept': kept,
      };
}
