-- Which renderer made a MIDI clip's playback copy (see src/midi_render.rs):
-- its settings and its SoundFont. Also set when that renderer looked at
-- the file and made nothing, because it has no notes or is too long, so
-- the file isn't tried again. NULL for every clip that isn't MIDI, and
-- for MIDI clips no renderer has seen. A MIDI clip whose version isn't
-- the running renderer's is rendered again at startup.
ALTER TABLE clips ADD COLUMN render_version TEXT;
