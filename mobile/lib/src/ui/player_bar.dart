import 'package:flutter/material.dart';

import '../format.dart';
import '../player/player_controller.dart';
import 'player_sheet.dart';
import 'waveform.dart';

/// The bar along the bottom, above every screen.
///
/// It lives outside the navigator, so moving between the list, a clip
/// and a wiki page never interrupts playback — the whole reason the web
/// version was rebuilt around a client-side router.
class PlayerBar extends StatelessWidget {
  const PlayerBar({super.key, required this.player, this.onOpenClip});

  final PlayerController player;

  /// Opens a clip's own screen, from the pane the bar raises. Null while
  /// there is nowhere to go.
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
        void open() => showPlayerSheet(
              context,
              player: player,
              onOpenClip: onOpenClip,
            );
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
                          // Everything but the play button and the
                          // waveform raises the pane. The waveform keeps
                          // seeking: a bar you cannot scrub would be a
                          // worse bar than one without a pane.
                          behavior: HitTestBehavior.opaque,
                          onTap: open,
                          child: Row(
                            children: [
                              Expanded(
                                child: Text(
                                  clip.name,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: theme.textTheme.bodyMedium,
                                ),
                              ),
                              if (player.repeat)
                                Icon(
                                  Icons.repeat_one,
                                  size: 14,
                                  color: theme.colorScheme.primary,
                                ),
                              Icon(
                                Icons.keyboard_arrow_up,
                                size: 18,
                                color: theme.colorScheme.outline,
                              ),
                            ],
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
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: open,
                    child: Text(
                      total == null
                          ? formatDuration(player.position)
                          : '${formatDuration(player.position)} / ${formatDuration(total)}',
                      style: theme.textTheme.labelSmall,
                    ),
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
