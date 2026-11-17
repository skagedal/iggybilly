/** Marks a clip that isn't a recording, so nobody presses play expecting one. */
export function FormatBadge({ format }: { format: string | null }) {
  if (format !== "midi") return null;
  return (
    <span className="format-badge" title="A MIDI file, not a recording">
      MIDI
    </span>
  );
}

/**
 * Shown where the player would be, for a clip with nothing to play: a
 * MIDI file with no notes, or one the server couldn't render.
 */
export function Unplayable() {
  return <p className="unplayable">This MIDI file can't be played here.</p>;
}

/** Under a MIDI clip's player, since it isn't what the file's author heard. */
export function RenderingNote({ format }: { format: string | null }) {
  if (format !== "midi") return null;
  return (
    <p className="rendering-note">
      Played with a general-purpose instrument set. It will sound different
      in your own setup.
    </p>
  );
}
