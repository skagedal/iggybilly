-- Precomputed waveform data so the UI can draw a clip's waveform without
-- downloading and decoding the audio. `peaks` is a compact JSON array of
-- normalised amplitude peaks (see src/waveform.rs); `duration_seconds`
-- lets WaveSurfer lay out the timeline without loading the media. Both
-- are nullable: older clips and any format we can't decode just don't
-- get a precomputed waveform (the player falls back to decoding on play).
ALTER TABLE clips ADD COLUMN peaks TEXT;
ALTER TABLE clips ADD COLUMN duration_seconds REAL;
