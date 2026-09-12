import 'package:flutter/material.dart';

import '../api/models.dart';
import '../cache/track_cache.dart';
import 'app_scope.dart';
import 'common.dart';

/// The switch that decides whether a clip stays on this phone.
///
/// Off, a clip is cached when it is played and deleted again when the
/// cache needs the room. On, it is downloaded now and kept until this is
/// turned off — and it stops counting against the cache ceiling, because
/// a limit on what the app may guess at should not be spent on what was
/// asked for.
///
/// One widget rather than one per screen, so that the clip's own page and
/// the player pane say the same thing and fail the same way. It is on both
/// because the moment you decide you want a clip with you is usually the
/// moment you are listening to it.
class KeepDownloadedTile extends StatelessWidget {
  const KeepDownloadedTile({super.key, required this.clip, this.dense = false});

  final Clip clip;

  /// Set in the player pane, where the row is one of several.
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final scope = AppScope.of(context);
    final cache = scope.cache;
    // Nowhere to keep anything, so nothing to promise.
    if (cache == null || !cache.isUsable) return const SizedBox.shrink();

    final url = scope.session.api.resolve(clip.audioPath);
    return ListenableBuilder(
      listenable: cache,
      builder: (context, _) {
        final busy = cache.isDownloading(url);
        return SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: dense,
          value: cache.isKept(url),
          secondary: busy
              ? const SizedBox(
                  width: 24,
                  height: 24,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : Icon(
                  cache.isKept(url)
                      ? Icons.download_done
                      : Icons.download_outlined,
                ),
          title: const Text('Keep downloaded'),
          subtitle: Text(_subtitle(cache, url, busy: busy)),
          // Off while the download runs, so a second tap cannot start a
          // second one or take back a promise that is not made yet.
          onChanged: busy ? null : (value) => _set(context, cache, url, value),
        );
      },
    );
  }

  Future<void> _set(
    BuildContext context,
    TrackCache cache,
    Uri url,
    bool value,
  ) async {
    final headers = AppScope.of(context).session.api.authHeaders;
    final kept = await cache.setKept(clip, url, value, headers: headers);
    // A switch that springs back on its own is the worst way to find out
    // there was no network.
    if (value && !kept && context.mounted) {
      showMessage(context, "Couldn't download that clip.", isError: true);
    }
  }

  static String _subtitle(TrackCache cache, Uri url, {required bool busy}) {
    if (busy) return 'Downloading…';
    if (cache.isKept(url)) {
      return 'Kept on this phone, and not counted against the cache.';
    }
    if (cache.hasCopy(url)) {
      return 'Cached for now; may be deleted to make room.';
    }
    return 'Download it and keep it for offline listening.';
  }
}
