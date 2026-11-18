import 'package:material_ui/material_ui.dart';

import '../api/models.dart';

/// Shown where the player would be, for a clip with nothing to play: a
/// MIDI file with no notes, or one the server couldn't render.
const unplayableMessage = "This MIDI file can't be played here.";

/// Under a MIDI clip's player, since it isn't what the file's author
/// heard.
const renderingNote =
    'Played with a general-purpose instrument set. '
    'It will sound different in your own setup.';

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
