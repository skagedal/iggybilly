-- The format detected from the stored file's bytes: mp4-aac, mp3 or
-- adts (see src/audio_format.rs). NULL until the startup pass has looked
-- at a clip stored before detection existed.
ALTER TABLE clips ADD COLUMN format TEXT;

-- A second file beside storage_filename in the audio dir, streamed in
-- place of it when present. Only formats some client can't play as
-- stored get one (ADTS, remuxed into MP4). storage_filename stays the
-- untouched upload, which Download serves, and content_type is its type.
ALTER TABLE clips ADD COLUMN playback_filename TEXT;
ALTER TABLE clips ADD COLUMN playback_content_type TEXT;
