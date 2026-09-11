import 'package:flutter/material.dart';

import '../format.dart';
import '../player/player_controller.dart';
import 'waveform.dart';

/// The bar along the bottom, above every screen.
///
/// It lives outside the navigator, so moving between the list, a clip
/// and a wiki page never interrupts playback — the whole reason the web
/// version was rebuilt around a client-side router.
class PlayerBar extends StatelessWidget {
  const PlayerBar({super.key, required this.player, this.onOpenClip});

  final PlayerController player;

  /// Tapping the name opens the clip. Null while there is nowhere to go.
  final ValueChanged<int>? onOpenClip;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: player,
      builder: (context, _) {
        final clip = player.clip;
        // Nothing loaded: take up no room at all rather than showing an
        // empty bar.
        if (clip == null) return const SizedBox.shrink();

        final theme = Theme.of(context);
        final total = player.duration;
        return Material(
          color: theme.colorScheme.surfaceContainerHigh,
          child: SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(4, 6, 12, 6),
              child: Row(
                children: [
                  IconButton(
                    onPressed: player.toggle,
                    tooltip: player.isPlaying ? 'Pause' : 'Play',
                    icon: Icon(
                      player.isPlaying ? Icons.pause : Icons.play_arrow,
                    ),
                  ),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        GestureDetector(
                          onTap: onOpenClip == null
                              ? null
                              : () => onOpenClip!(clip.id),
                          child: Text(
                            clip.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodyMedium,
                          ),
                        ),
                        const SizedBox(height: 2),
                        Waveform(
                          peaks: clip.peaks,
                          progress: player.progress,
                          height: 22,
                          onSeek: player.seekFraction,
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 10),
                  Text(
                    total == null
                        ? formatDuration(player.position)
                        : '${formatDuration(player.position)} / ${formatDuration(total)}',
                    style: theme.textTheme.labelSmall,
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}
