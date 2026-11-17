//! Rendering a MIDI file to MP3: the playback copy of a MIDI clip.
//!
//! See `specs/implemented/004-midi-files.md`. The sequence is played
//! through a SoundFont with rustysynth and encoded with LAME, both in
//! process, so a MIDI clip ends up as an ordinary audio file that every
//! client already knows how to play, cache and loop.

use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Quality};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use sha2::{Digest, Sha256};

/// Bumped whenever the output changes for the same SoundFont: a new
/// sample rate, bitrate, tail rule. Part of [`Renderer::version`], so
/// bumping it re-renders every MIDI clip on the next start.
const SETTINGS_VERSION: u32 = 1;

const SAMPLE_RATE: u32 = 44_100;

/// Longer than this is almost certainly a mistake in the file, and would
/// hold the upload for a long time.
const MAX_SECONDS: f64 = 30.0 * 60.0;

/// After the last event, keep rendering while the release tails ring
/// out, until a block is 60 dB below the loudest moment or this much
/// time has passed. Reverb takes far longer than that to reach true
/// silence, and seconds of near-nothing at the end of a clip are worse
/// than a slightly early cut, especially when it loops.
const TAIL_SILENCE: f32 = 1.0e-3;
const MAX_TAIL_SECONDS: f64 = 5.0;

/// Where the loudest sample lands after normalising: -1 dBFS, leaving
/// the MP3 encoder a little headroom. rustysynth's own level peaks well
/// below a recording's, and a MIDI clip should sit beside them in a
/// playlist without a jump in volume.
const TARGET_PEAK: f32 = 0.891;
/// Don't turn a nearly silent file into noise.
const MAX_GAIN: f32 = 8.0;

const BLOCK: usize = 4096;

pub const CONTENT_TYPE: &str = "audio/mpeg";

/// A loaded SoundFont, shared by every render.
pub struct Renderer {
    sound_font: Arc<SoundFont>,
    version: String,
}

/// Why a MIDI file got no playback copy.
#[derive(Debug)]
pub enum Declined {
    NoNotes,
    TooLong { seconds: f64 },
}

impl Renderer {
    /// Load the SoundFont at `path`. Takes a moment and about as much
    /// memory as the file, so it is done once at startup.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("reading SoundFont {}", path.display()))?;
        let digest = Sha256::digest(&bytes);
        let sound_font = SoundFont::new(&mut bytes.as_slice())
            .map_err(|e| anyhow!("{e:?}"))
            .with_context(|| format!("parsing SoundFont {}", path.display()))?;
        let hash: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self {
            sound_font: Arc::new(sound_font),
            version: format!("{SETTINGS_VERSION}-{hash}"),
        })
    }

    /// Identifies what a render sounds like: these settings and this
    /// SoundFont. A MIDI clip records the version of its copy, and one
    /// made by any other version is rendered again.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Render the MIDI file at `src` to an MP3 at `dst`. A file that is
    /// declined writes nothing.
    pub fn render(&self, src: &Path, dst: &Path) -> anyhow::Result<Result<(), Declined>> {
        let bytes = std::fs::read(src).context("reading MIDI file")?;
        if !has_notes(&bytes) {
            return Ok(Err(Declined::NoNotes));
        }
        let midi = MidiFile::new(&mut bytes.as_slice()).map_err(|e| anyhow!("{e:?}"))?;
        let seconds = midi.get_length();
        if seconds > MAX_SECONDS {
            return Ok(Err(Declined::TooLong { seconds }));
        }

        let midi = Arc::new(midi);
        let end = (seconds * SAMPLE_RATE as f64) as usize;
        let max_end = end + (MAX_TAIL_SECONDS * SAMPLE_RATE as f64) as usize;

        // Two passes over the same deterministic synthesis: one to find
        // the peak and where the tail fades out, one to encode at the
        // gain that peak calls for. Rendering is cheap next to encoding,
        // and holding a whole render in memory to normalise it isn't, at
        // up to thirty minutes of stereo.
        let mut left = vec![0f32; BLOCK];
        let mut right = vec![0f32; BLOCK];
        let mut sequencer = self.sequencer(&midi)?;
        let mut peak = 0f32;
        let mut rendered = 0usize;
        while rendered < end {
            sequencer.render(&mut left, &mut right);
            peak = peak.max(block_peak(&left, &right));
            rendered += BLOCK;
        }
        while rendered < max_end {
            sequencer.render(&mut left, &mut right);
            rendered += BLOCK;
            if block_peak(&left, &right) < peak * TAIL_SILENCE {
                break;
            }
        }
        let total = rendered;
        let gain = if peak > 0.0 {
            (TARGET_PEAK / peak).min(MAX_GAIN)
        } else {
            1.0
        };
        let mut sequencer = self.sequencer(&midi)?;

        let mut encoder = Builder::new()
            .ok_or_else(|| anyhow!("couldn't start LAME"))?
            .with_num_channels(2)
            .and_then(|b| b.with_sample_rate(SAMPLE_RATE))
            .and_then(|b| b.with_brate(Bitrate::Kbps192))
            .and_then(|b| b.with_quality(Quality::NearBest))
            .and_then(|b| b.with_to_write_vbr_tag(true))
            .and_then(|b| b.build())
            .map_err(|e| anyhow!("{e:?}"))?;

        let mut file = BufWriter::new(std::fs::File::create(dst).context("creating MP3 file")?);
        let mut mp3 = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(BLOCK));
        let mut rendered = 0usize;
        while rendered < total {
            sequencer.render(&mut left, &mut right);
            for s in left.iter_mut().chain(right.iter_mut()) {
                *s = (*s * gain).clamp(-1.0, 1.0);
            }
            mp3.clear();
            encoder
                .encode_to_vec(
                    DualPcm {
                        left: &left,
                        right: &right,
                    },
                    &mut mp3,
                )
                .map_err(|e| anyhow!("{e:?}"))?;
            file.write_all(&mp3)?;
            rendered += BLOCK;
        }
        mp3.clear();
        encoder
            .flush_to_vec::<FlushNoGap>(&mut mp3)
            .map_err(|e| anyhow!("{e:?}"))?;
        file.write_all(&mp3)?;

        // The first frame was left empty for the LAME tag, which carries
        // the length and the encoder delay. Players use it for an exact
        // duration and a gapless loop.
        mp3.clear();
        mp3.reserve(encoder.lame_tag_size());
        if encoder.lame_tag_encode_to_vec(&mut mp3).is_some() {
            file.seek(SeekFrom::Start(encoder.id3v2_tag_size() as u64))?;
            file.write_all(&mp3)?;
        }
        file.flush()?;
        Ok(Ok(()))
    }

    fn sequencer(&self, midi: &Arc<MidiFile>) -> anyhow::Result<MidiFileSequencer> {
        let settings = SynthesizerSettings::new(SAMPLE_RATE as i32);
        let synthesizer =
            Synthesizer::new(&self.sound_font, &settings).map_err(|e| anyhow!("{e:?}"))?;
        let mut sequencer = MidiFileSequencer::new(synthesizer);
        sequencer.play(midi, false);
        Ok(sequencer)
    }
}

fn block_peak(left: &[f32], right: &[f32]) -> f32 {
    left.iter().chain(right).fold(0f32, |m, s| m.max(s.abs()))
}

/// Some exports carry only a tempo map, which would render as silence.
fn has_notes(bytes: &[u8]) -> bool {
    let Ok(smf) = midly::Smf::parse(bytes) else {
        return false;
    };
    smf.tracks.iter().flatten().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Midi {
                message: midly::MidiMessage::NoteOn { vel, .. },
                ..
            } if vel > 0
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/midi")
            .join(name)
    }

    #[test]
    fn renders_an_mp3_as_long_as_the_sequence() {
        let renderer = Renderer::load(&fixture("tiny.sf2")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.mp3");
        renderer
            .render(&fixture("riff.mid"), &out)
            .unwrap()
            .unwrap();

        assert_eq!(
            crate::audio_format::detect(&out).unwrap(),
            Ok(crate::audio_format::AudioFormat::Mp3)
        );
        // Four quarter notes at 120 BPM, plus the release.
        let waveform = crate::audio::compute(&out).expect("decodes");
        assert!(
            (2.0..3.5).contains(&waveform.duration_seconds),
            "{}",
            waveform.duration_seconds
        );
        assert!(waveform.peaks.iter().any(|&p| p > 0.05), "not silent");
    }

    #[test]
    fn a_tempo_map_alone_is_declined() {
        let renderer = Renderer::load(&fixture("tiny.sf2")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.mp3");
        let outcome = renderer.render(&fixture("tempo-only.mid"), &out).unwrap();
        assert!(matches!(outcome, Err(Declined::NoNotes)));
        assert!(!out.exists());
    }

    #[test]
    fn the_version_names_the_sound_font() {
        let renderer = Renderer::load(&fixture("tiny.sf2")).unwrap();
        assert!(renderer.version().starts_with("1-"));
        assert_eq!(renderer.version().len(), "1-".len() + 12);
    }
}
