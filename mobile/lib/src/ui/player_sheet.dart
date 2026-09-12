import 'package:flutter/material.dart';

import '../cache/track_cache.dart';
import '../format.dart';
import '../player/player_controller.dart';
import 'app_scope.dart';
import 'keep_downloaded.dart';
import 'waveform.dart';

/// Raise the player pane over whatever is on screen.
///
/// A modal sheet on the root navigator, so it covers the bar that raised
/// it rather than sitting above it — the pane *is* the bar, opened up.
Future<void> showPlayerSheet(
  BuildContext context, {
  required PlayerController player,
  ValueChanged<int>? onOpenClip,
}) {
  final scope = AppScope.of(context);
  return showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    // The sheet is as tall as its contents, but asking for scroll control
    // is what lets it exceed half the screen on a short phone.
    isScrollControlled: true,
    builder: (_) => PlayerSheet(
      player: player,
      cache: scope.cache,
      onOpenClip: onOpenClip,
    ),
  );
}

/// The player, opened up: the whole waveform, the times, and the
/// decisions that do not fit on one line of the bar.
class PlayerSheet extends StatelessWidget {
  const PlayerSheet({
    super.key,
    required this.player,
    this.cache,
    this.onOpenClip,
  });

  final PlayerController player;
  final TrackCache? cache;
  final ValueChanged<int>? onOpenClip;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SafeArea(
      top: false,
      child: ListenableBuilder(
        listenable: player,
        builder: (context, _) {
          final clip = player.clip;
          // Deleting the loaded clip closes the pane, rather than leaving
          // a player for something that no longer exists.
          if (clip == null) return const SizedBox.shrink();

          final total = player.duration;
          return Padding(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  clip.name,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.center,
                  style: theme.textTheme.titleLarge,
                ),
                const SizedBox(height: 4),
                Text(
                  _subtitle(clip.uploader, clip.recordingDate),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall
                      ?.copyWith(color: theme.colorScheme.outline),
                ),
                const SizedBox(height: 20),
                Waveform(
                  peaks: clip.peaks,
                  progress: player.progress,
                  height: 80,
                  onSeek: player.seekFraction,
                ),
                const SizedBox(height: 6),
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      formatDuration(player.position),
                      style: theme.textTheme.labelMedium,
                    ),
                    Text(
                      total == null ? '—' : formatDuration(total),
                      style: theme.textTheme.labelMedium,
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                _Controls(player: player),
                const SizedBox(height: 8),
                KeepDownloadedTile(clip: clip, dense: true),
                if (onOpenClip != null)
                  Align(
                    alignment: Alignment.center,
                    child: TextButton.icon(
                      onPressed: () {
                        // Close first: the clip's own screen is pushed
                        // onto the navigator underneath this sheet.
                        Navigator.of(context).pop();
                        onOpenClip!(clip.id);
                      },
                      icon: const Icon(Icons.open_in_new, size: 18),
                      label: const Text('Open clip'),
                    ),
                  ),
              ],
            ),
          );
        },
      ),
    );
  }

  static String _subtitle(String uploader, String? recordingDate) {
    final parts = <String>[
      if (uploader.isNotEmpty) uploader,
      if (recordingDate != null) 'rec. $recordingDate',
    ];
    return parts.join(' · ');
  }
}

class _Controls extends StatelessWidget {
  const _Controls({required this.player});

  final PlayerController player;

  /// How far the skip buttons jump. Ten seconds is the length of a phrase
  /// someone is trying to hear again, which is what this player is for.
  static const _skip = Duration(seconds: 10);

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    // Evenly spaced rather than centred with a counterweight: four
    // buttons spread across the row reads as a row of controls, and needs
    // no invisible box to keep play in the middle.
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceEvenly,
      children: [
        IconButton(
          tooltip: player.repeat ? 'Stop repeating' : 'Repeat',
          onPressed: player.toggleRepeat,
          icon: Icon(player.repeat ? Icons.repeat_one : Icons.repeat),
          color: player.repeat ? theme.colorScheme.primary : null,
        ),
        IconButton(
          tooltip: 'Back 10 seconds',
          iconSize: 30,
          onPressed: () => player.skip(-_skip),
          icon: const Icon(Icons.replay_10),
        ),
        IconButton.filled(
          iconSize: 44,
          tooltip: player.isPlaying ? 'Pause' : 'Play',
          onPressed: player.toggle,
          icon: Icon(player.isPlaying ? Icons.pause : Icons.play_arrow),
        ),
        IconButton(
          tooltip: 'Forward 10 seconds',
          iconSize: 30,
          onPressed: () => player.skip(_skip),
          icon: const Icon(Icons.forward_10),
        ),
      ],
    );
  }
}
