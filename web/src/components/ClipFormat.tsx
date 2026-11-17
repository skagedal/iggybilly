/** Marks a clip that isn't a recording, so nobody presses play expecting one. */
export function FormatBadge({ format }: { format: string | null }) {
  if (format !== "midi") return null;
  return (
    <span className="format-badge" title="A MIDI file, not a recording">
      MIDI
    </span>
  );
}

/** Shown where the player would be, for a clip with nothing to play. */
export function Unplayable() {
  return <p className="unplayable">Playback for MIDI files is coming.</p>;
}
