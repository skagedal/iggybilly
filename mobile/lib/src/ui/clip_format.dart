import 'package:flutter/material.dart';

import '../api/models.dart';

/// Shown where the player would be, for a clip with nothing to play.
const unplayableMessage = 'Playback for MIDI files is coming.';

/// Marks a clip that isn't a recording, so nobody presses play expecting
/// one. Nothing for a recording.
class FormatBadge extends StatelessWidget {
  const FormatBadge({super.key, required this.clip});

  final Clip clip;

  @override
  Widget build(BuildContext context) {
    if (!clip.isMidi) return const SizedBox.shrink();
    final theme = Theme.of(context);
    final colour = theme.colorScheme.outline;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      decoration: BoxDecoration(
        border: Border.all(color: theme.colorScheme.outlineVariant),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(
        'MIDI',
        semanticsLabel: 'MIDI file',
        style: theme.textTheme.labelSmall?.copyWith(
          color: colour,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.5,
        ),
      ),
    );
  }
}
