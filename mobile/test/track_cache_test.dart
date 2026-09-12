import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:iggybilly/src/cache/track_cache.dart';

import 'player_controller_test.dart' show clipFixture;

/// A server that hands out clip audio, and can be told to misbehave.
class FakeAudioServer extends http.BaseClient {
  /// Body per path, e.g. `/clips/1/audio`.
  final Map<String, List<int>> bodies = {};
  final List<http.BaseRequest> requests = [];

  /// Answered instead of 200 when set, for the "the server said no" case.
  int? status;

  /// What to claim in `content-length`, given the body actually sent. The
  /// default is the truth; a test that lies here is testing that a
  /// truncated download is thrown away.
  int? Function(List<int> body)? claimedLength;

  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) async {
    requests.add(request);
    final body = bodies[request.url.path];
    if (body == null || status != null) {
      return http.StreamedResponse(
        const Stream<List<int>>.empty(),
        body == null ? 404 : status!,
      );
    }
    return http.StreamedResponse(
      Stream.value(body),
      200,
      contentLength: claimedLength == null ? body.length : claimedLength!(body),
    );
  }
}

/// A body of [bytes] bytes, distinguishable from another clip's.
List<int> audioOf(int bytes, {int fill = 7}) =>
    List<int>.filled(bytes, fill, growable: false);

void main() {
  late Directory directory;
  late FakeAudioServer server;
  late TrackCache cache;

  Uri urlFor(int id) => Uri.parse('https://example.test/clips/$id/audio');

  TrackCache open({Directory? at}) => TrackCache(
        locate: () async => at ?? directory,
        httpClient: server,
      );

  /// Put [bytes] bytes on the fake server as clip [id], then cache it.
  Future<void> store(int id, int bytes, {bool kept = false}) async {
    server.bodies['/clips/$id/audio'] = audioOf(bytes, fill: id);
    final file = await cache.store(
      clipFixture(id: id, name: 'clip $id'),
      urlFor(id),
      kept: kept,
    );
    expect(file, isNotNull, reason: 'clip $id should have been cached');
  }

  setUp(() async {
    directory = await Directory.systemTemp.createTemp('iggybilly-cache-test');
    server = FakeAudioServer();
    cache = open();
    await cache.open();
  });

  tearDown(() async {
    if (await directory.exists()) {
      await directory.delete(recursive: true);
    }
  });

  test('a stored clip is on disk and found again', () async {
    server.bodies['/clips/1/audio'] = audioOf(2048);
    final file = await cache.store(
      clipFixture(),
      urlFor(1),
      headers: {'Authorization': 'Bearer tok'},
    );

    expect(file, isNotNull);
    expect(await file!.length(), 2048);
    expect(cache.fileFor(urlFor(1))!.path, file.path);
    expect(cache.hasCopy(urlFor(1)), isTrue);
    expect(cache.usedBytes, 2048);

    // Audio is an authenticated endpoint; a cache that fetched it without
    // the token would silently store 401 pages.
    expect(server.requests.single.headers['Authorization'], 'Bearer tok');
  });

  test('the extension follows the content type, which AVPlayer reads',
      () async {
    server.bodies['/clips/1/audio'] = audioOf(16);
    final file = await cache.store(clipFixture(), urlFor(1));
    expect(file!.path, endsWith('.mp3'));
  });

  test('a second ask for the same clip is the same download', () async {
    server.bodies['/clips/1/audio'] = audioOf(64);
    final first = cache.store(clipFixture(), urlFor(1));
    final second = cache.store(clipFixture(), urlFor(1));
    await Future.wait([first, second]);

    expect(server.requests.length, 1, reason: 'not fetched twice');
    expect(cache.usedBytes, 64);
  });

  test('the least recently played clip is the one that goes', () async {
    await cache.setMaxBytes(3000);
    await store(1, 1000);
    await store(2, 1000);
    await store(3, 1000);
    // Playing 1 again puts 2 at the back of the queue.
    await cache.touch(clipFixture(id: 1), urlFor(1));

    await store(4, 1000);

    expect(cache.hasCopy(urlFor(2)), isFalse, reason: 'played longest ago');
    expect(cache.hasCopy(urlFor(1)), isTrue);
    expect(cache.hasCopy(urlFor(3)), isTrue);
    expect(cache.hasCopy(urlFor(4)), isTrue);
    expect(cache.usedBytes, 3000);
    // The bytes are gone, not just the bookkeeping.
    expect(directory.listSync().whereType<File>().length, 4,
        reason: 'three clips and the index');
  });

  test('a kept clip is neither counted nor evicted', () async {
    await cache.setMaxBytes(2000);
    // Kept, and the oldest thing here: eviction by age would take it
    // first, which is exactly what keeping it has to prevent.
    await store(1, 1000, kept: true);
    await store(2, 1000);
    await store(3, 1000);
    await store(4, 1000);

    expect(cache.isKept(urlFor(1)), isTrue);
    expect(cache.hasCopy(urlFor(1)), isTrue, reason: 'kept clips stay');
    expect(cache.hasCopy(urlFor(2)), isFalse, reason: 'oldest unkept');
    expect(cache.keptBytes, 1000);
    expect(cache.usedBytes, 2000, reason: 'the kept clip is not in this');
  });

  test('keeping a clip that is not here yet downloads it', () async {
    server.bodies['/clips/9/audio'] = audioOf(512);
    expect(await cache.setKept(clipFixture(id: 9), urlFor(9), true), isTrue);

    expect(cache.isKept(urlFor(9)), isTrue);
    expect(cache.fileFor(urlFor(9)), isNotNull);
    expect(cache.keptBytes, 512);
    expect(cache.usedBytes, 0);
  });

  test('a clip already cached can be kept without fetching it again',
      () async {
    await store(1, 256);
    await cache.setKept(clipFixture(id: 1), urlFor(1), true);

    expect(server.requests.length, 1);
    expect(cache.keptBytes, 256);
  });

  test('no longer keeping a clip leaves the file, at risk', () async {
    await cache.setMaxBytes(1000);
    await store(1, 800, kept: true);
    await store(2, 800);
    expect(cache.hasCopy(urlFor(1)), isTrue);

    await cache.setKept(clipFixture(id: 1), urlFor(1), false);

    // 1600 unkept bytes against a 1000-byte ceiling: one of them has to
    // go, and it is the one played longest ago.
    expect(cache.keptBytes, 0);
    expect(cache.hasCopy(urlFor(1)), isFalse);
    expect(cache.hasCopy(urlFor(2)), isTrue);
  });

  test('stopping keeping from the storage list does the same', () async {
    await store(1, 128, kept: true);
    final track = cache.keptTracks.single;
    expect(track.name, 'clip 1');
    expect(track.bytes, 128);

    await cache.stopKeeping(track);
    expect(cache.keptTracks, isEmpty);
    expect(cache.hasCopy(urlFor(1)), isTrue, reason: 'cached, not deleted');
  });

  test('lowering the ceiling evicts straight away', () async {
    await store(1, 1000);
    await store(2, 1000);
    expect(cache.usedBytes, 2000);

    await cache.setMaxBytes(1200);
    expect(cache.usedBytes, 1000);
    expect(cache.hasCopy(urlFor(2)), isTrue, reason: 'the newer one stays');
  });

  test('clearing the cache spares the clips that were kept', () async {
    await store(1, 100, kept: true);
    await store(2, 100);

    await cache.clearCache();

    expect(cache.hasCopy(urlFor(1)), isTrue);
    expect(cache.hasCopy(urlFor(2)), isFalse);
    expect(cache.usedBytes, 0);
    expect(cache.keptBytes, 100);
  });

  test('forgetting a clip deletes it whether it was kept or not', () async {
    await store(1, 100, kept: true);
    final file = cache.fileFor(urlFor(1))!;

    await cache.forget(urlFor(1));

    expect(cache.hasCopy(urlFor(1)), isFalse);
    expect(file.existsSync(), isFalse);
  });

  test('what is on disk, and the ceiling, survive a restart', () async {
    await cache.setMaxBytes(TrackCache.sizeChoices.first);
    await store(1, 300, kept: true);
    await store(2, 300);

    final reopened = open();
    await reopened.open();

    expect(reopened.maxBytes, TrackCache.sizeChoices.first);
    expect(reopened.fileFor(urlFor(1)), isNotNull);
    expect(reopened.isKept(urlFor(1)), isTrue);
    expect(reopened.usedBytes, 300);
    expect(reopened.keptBytes, 300);
  });

  test('a clip the OS deleted behind our back is forgotten', () async {
    await store(1, 300);
    cache.fileFor(urlFor(1))!.deleteSync();

    final reopened = open();
    await reopened.open();

    expect(reopened.hasCopy(urlFor(1)), isFalse);
    expect(reopened.usedBytes, 0);
  });

  test('a file nothing points at is cleaned up on the way in', () async {
    final orphan = File('${directory.path}/left-behind.mp3')
      ..writeAsBytesSync(audioOf(64));

    final reopened = open();
    await reopened.open();

    expect(orphan.existsSync(), isFalse);
  });

  test('an unreadable index starts empty rather than throwing', () async {
    await store(1, 300);
    File('${directory.path}/index.json').writeAsStringSync('{oh dear');

    final reopened = open();
    await reopened.open();

    expect(reopened.isUsable, isTrue);
    expect(reopened.hasCopy(urlFor(1)), isFalse);
    // The clip's bytes were unaccounted for, so they went with the index.
    expect(directory.listSync().whereType<File>().map((f) => f.path),
        everyElement(endsWith('index.json')));
  });

  test('keeping a clip the server will not hand over reports that', () async {
    // Nothing on the server for clip 9: what an offline phone looks like.
    expect(await cache.setKept(clipFixture(id: 9), urlFor(9), false), isFalse);
    expect(await cache.setKept(clipFixture(id: 9), urlFor(9), true), isFalse,
        reason: 'the switch has to spring back, and say why');
    expect(cache.isKept(urlFor(9)), isFalse);
  });

  test('a clip being fetched says so while it is in flight', () async {
    server.bodies['/clips/1/audio'] = audioOf(64);
    expect(cache.isDownloading(urlFor(1)), isFalse);

    final download = cache.store(clipFixture(), urlFor(1));
    expect(cache.isDownloading(urlFor(1)), isTrue);

    await download;
    expect(cache.isDownloading(urlFor(1)), isFalse);
  });

  test('a truncated download is thrown away, not stored', () async {
    // The body arrives short of what the server promised: a phone that
    // lost its signal halfway through.
    server.claimedLength = (body) => body.length + 10;
    server.bodies['/clips/1/audio'] = audioOf(500);

    expect(await cache.store(clipFixture(), urlFor(1)), isNull);
    expect(cache.hasCopy(urlFor(1)), isFalse);
    expect(cache.usedBytes, 0);
    // Nothing half-written left behind for the player to trip over.
    expect(directory.listSync().whereType<File>().length, 1);
  });

  test('a server that says no leaves nothing behind', () async {
    server.bodies['/clips/1/audio'] = audioOf(500);
    server.status = 500;

    expect(await cache.store(clipFixture(), urlFor(1)), isNull);
    expect(cache.hasCopy(urlFor(1)), isFalse);
    expect(directory.listSync().whereType<File>().length, 1);
  });

  test('a clip the server does not have is a miss, not a crash', () async {
    expect(await cache.store(clipFixture(id: 404), urlFor(404)), isNull);
    expect(cache.usedBytes, 0);
  });

  test('nowhere to write makes every lookup a miss', () async {
    final unusable = TrackCache(
      locate: () async => throw const FileSystemException('no'),
      httpClient: server,
    );
    await unusable.open();

    expect(unusable.isUsable, isFalse);
    expect(unusable.fileFor(urlFor(1)), isNull);
    server.bodies['/clips/1/audio'] = audioOf(10);
    expect(await unusable.store(clipFixture(), urlFor(1)), isNull);
  });

  test('the same clip id on two servers is two entries', () async {
    server.bodies['/clips/1/audio'] = audioOf(100);
    await cache.store(clipFixture(), urlFor(1));
    await cache.store(clipFixture(), Uri.parse('https://other.test/clips/1/audio'));

    expect(cache.usedBytes, 200);
    expect(cache.fileFor(urlFor(1))!.path,
        isNot(cache.fileFor(Uri.parse('https://other.test/clips/1/audio'))!.path));
  });

  test('the download query does not make a second entry', () async {
    server.bodies['/clips/1/audio'] = audioOf(100);
    await cache.store(clipFixture(), urlFor(1));

    expect(
      cache.hasCopy(Uri.parse('https://example.test/clips/1/audio?download=1')),
      isTrue,
    );
  });
}
