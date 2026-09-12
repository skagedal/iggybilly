import 'package:flutter/material.dart';

import '../cache/track_cache.dart';
import '../format.dart';
import 'common.dart';

/// What the app is keeping on this phone, and how much of it it may keep.
///
/// Two numbers rather than one, because they mean different things. The
/// cache is the app's own guess at what will be wanted again and is
/// bounded by a ceiling the user sets. A kept clip was asked for, so it
/// is not counted against that ceiling and is not evicted — deleting it
/// takes a deliberate act, here or on the clip.
class StoragePage extends StatelessWidget {
  const StoragePage({super.key, required this.cache});

  final TrackCache cache;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Downloads')),
      body: ListenableBuilder(
        listenable: cache,
        builder: (context, _) {
          if (!cache.isUsable) {
            return const EmptyBody(
              icon: Icons.sd_card_alert_outlined,
              message: 'This phone would not give the app somewhere to keep '
                  'clips, so every clip streams.',
            );
          }
          return ListView(
            padding: const EdgeInsets.only(bottom: 40),
            children: [
              _Usage(cache: cache),
              const Divider(),
              _MaxSize(cache: cache),
              const Divider(),
              _Kept(cache: cache),
            ],
          );
        },
      ),
    );
  }
}

class _Usage extends StatelessWidget {
  const _Usage({required this.cache});

  final TrackCache cache;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final used = cache.usedBytes;
    final max = cache.maxBytes;
    final kept = cache.keptBytes;

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Cache', style: theme.textTheme.titleSmall),
          const SizedBox(height: 6),
          ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: LinearProgressIndicator(
              value: max <= 0 ? 0 : (used / max).clamp(0.0, 1.0),
              minHeight: 8,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            '${formatBytes(used)} of ${formatBytes(max)} · '
            '${cache.cachedCount} ${cache.cachedCount == 1 ? 'clip' : 'clips'}',
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.outline),
          ),
          const SizedBox(height: 2),
          Text(
            kept == 0
                ? 'Nothing kept. Kept clips are stored separately and are not '
                    'counted here.'
                : '${formatBytes(kept)} kept as well, which is not counted '
                    'against the cache.',
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.outline),
          ),
          const SizedBox(height: 10),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton.tonal(
              onPressed: used == 0 ? null : () => _clear(context),
              child: const Text('Clear cache'),
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _clear(BuildContext context) async {
    final sure = await confirm(
      context,
      title: 'Clear the cache?',
      message: 'Cached clips are downloaded again next time they are '
          'played. Clips you have kept are left alone.',
      confirmLabel: 'Clear',
    );
    if (!sure) return;
    await cache.clearCache();
  }
}

class _MaxSize extends StatelessWidget {
  const _MaxSize({required this.cache});

  final TrackCache cache;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Maximum cache size', style: theme.textTheme.titleSmall),
          const SizedBox(height: 2),
          Text(
            'Once the cache passes this, the clip nobody has played for '
            'longest is deleted.',
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.outline),
          ),
          const SizedBox(height: 10),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final choice in TrackCache.sizeChoices)
                ChoiceChip(
                  label: Text(formatBytes(choice)),
                  selected: cache.maxBytes == choice,
                  onSelected: (_) => cache.setMaxBytes(choice),
                ),
            ],
          ),
          // A size that isn't one of the choices, because it was set
          // before this list changed. Shown rather than silently ignored.
          if (!TrackCache.sizeChoices.contains(cache.maxBytes)) ...[
            const SizedBox(height: 8),
            Text(
              'Currently ${formatBytes(cache.maxBytes)}.',
              style: theme.textTheme.bodySmall,
            ),
          ],
        ],
      ),
    );
  }
}

class _Kept extends StatelessWidget {
  const _Kept({required this.cache});

  final TrackCache cache;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final kept = cache.keptTracks;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: Text('Kept on this phone', style: theme.textTheme.titleSmall),
        ),
        if (kept.isEmpty)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 12),
            child: Text(
              'Turn on “Keep downloaded” on a clip and it will play with no '
              'network at all.',
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
          )
        else
          for (final track in kept)
            ListTile(
              leading: const Icon(Icons.download_done),
              title: Text(
                track.name.isEmpty ? 'Clip ${track.clipId}' : track.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
              subtitle: Text(
                '${formatBytes(track.bytes)} · '
                'played ${formatWhen(track.lastPlayedAt)}',
              ),
              trailing: IconButton(
                tooltip: 'Stop keeping this clip',
                icon: const Icon(Icons.close),
                // Not deleted: it drops into the cache, where it takes
                // its chances with everything else.
                onPressed: () => cache.stopKeeping(track),
              ),
            ),
      ],
    );
  }
}
