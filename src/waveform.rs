//! Precompute waveform peak data from an audio file so the web UI can
//! draw a waveform without downloading and decoding the audio in the
//! browser. We decode (pure Rust, via symphonia), mix to mono, and
//! reduce to a fixed-size array of normalised amplitude peaks plus the
//! clip's duration. The result is stored per clip and handed to
//! WaveSurfer's `peaks`/`duration` options.

use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Number of peaks we reduce a clip down to. WaveSurfer interpolates to
/// the container width, so this just needs to be finer than the widest
/// waveform we render. ~500 keeps the stored JSON small (a few KB).
const TARGET_PEAKS: usize = 500;

/// Frames per coarse bucket while streaming. We build a fine-grained peak
/// array first (bounded: ~1 value per 1024 frames, so a 10-minute clip is
/// ~28k floats) then resample it down to TARGET_PEAKS. This avoids holding
/// every decoded sample in memory or needing the total frame count up
/// front.
const WINDOW: usize = 1024;

pub struct Waveform {
    /// Normalised amplitude peaks in [0.0, 1.0], length <= TARGET_PEAKS.
    pub peaks: Vec<f32>,
    pub duration_seconds: f64,
}

/// Decode `path` and compute its waveform. Returns `None` for anything
/// we can't decode (e.g. opus, which symphonia doesn't support) or on
/// any read/decode error — callers treat a missing waveform as benign.
pub fn compute(path: &Path) -> Option<Waveform> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;

    // Pull the channel count and sample rate off the track before the
    // decode loop, so the immutable borrow of `format` is released before
    // we start calling `next_packet` (which needs it mutably).
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let audio_params = track.codec_params.as_ref()?.audio()?;
    let channels = audio_params.channels.as_ref().map_or(1, |c| c.count()).max(1);
    let sample_rate = audio_params.sample_rate.unwrap_or(44_100) as f64;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .ok()?;

    let mut fine: Vec<f32> = Vec::new();
    let mut window_max = 0f32;
    let mut window_count = 0usize;
    let mut total_frames: u64 = 0;
    let mut samples: Vec<f32> = Vec::new();

    // Ends on end-of-stream (Ok(None)) or any read error (Err) — either
    // way we stop with the frames decoded so far.
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let audio_buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // Recoverable: skip this packet and keep going.
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => continue,
            Err(_) => break,
        };

        // Copy this packet's samples out as channel-interleaved f32.
        samples.resize(audio_buf.samples_interleaved(), 0.0);
        audio_buf.copy_to_slice_interleaved(samples.as_mut_slice());

        for frame in samples.chunks(channels) {
            let mono: f32 = frame.iter().copied().sum::<f32>() / channels as f32;
            window_max = window_max.max(mono.abs());
            window_count += 1;
            total_frames += 1;
            if window_count == WINDOW {
                fine.push(window_max);
                window_max = 0.0;
                window_count = 0;
            }
        }
    }
    if window_count > 0 {
        fine.push(window_max);
    }
    if total_frames == 0 {
        return None;
    }

    Some(Waveform {
        peaks: resample_and_normalise(&fine),
        duration_seconds: total_frames as f64 / sample_rate,
    })
}

/// Reduce the fine peak array to at most TARGET_PEAKS buckets (max within
/// each bucket) and scale so the loudest peak is 1.0.
fn resample_and_normalise(fine: &[f32]) -> Vec<f32> {
    let mut peaks: Vec<f32> = if fine.len() <= TARGET_PEAKS {
        fine.to_vec()
    } else {
        (0..TARGET_PEAKS)
            .map(|i| {
                let start = i * fine.len() / TARGET_PEAKS;
                let end = ((i + 1) * fine.len() / TARGET_PEAKS).max(start + 1);
                fine[start..end].iter().copied().fold(0.0f32, f32::max)
            })
            .collect()
    };

    let max = peaks.iter().copied().fold(0.0f32, f32::max);
    if max > 0.0 {
        for p in &mut peaks {
            *p /= max;
        }
    }
    peaks
}

/// Serialise peaks as a compact JSON array (values rounded to 3 decimals)
/// for storage and for embedding in a data attribute. Contains only
/// digits, `.`, `,`, `-` and brackets, so it's safe unquoted in HTML.
pub fn peaks_to_json(peaks: &[f32]) -> String {
    let mut s = String::with_capacity(peaks.len() * 5 + 2);
    s.push('[');
    for (i, p) in peaks.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let rounded = (p * 1000.0).round() / 1000.0;
        s.push_str(&rounded.to_string());
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn computes_waveform_for_m4a_fixture() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guitar-clip.m4a");
        let wf = compute(&p).expect("should decode the m4a fixture");

        // The clip is ~2s; allow slack for codec priming/padding.
        assert!(
            (wf.duration_seconds - 2.0).abs() < 0.5,
            "duration {} not ~2s",
            wf.duration_seconds
        );
        assert!(!wf.peaks.is_empty() && wf.peaks.len() <= TARGET_PEAKS);
        // Normalised to [0, 1] with the loudest peak reaching 1.0.
        assert!(wf.peaks.iter().all(|p| *p >= 0.0 && *p <= 1.0));
        let max = wf.peaks.iter().copied().fold(0.0f32, f32::max);
        assert!((max - 1.0).abs() < 1e-3, "peaks should be normalised, max={max}");
    }

    #[test]
    fn peaks_json_is_compact_and_attribute_safe() {
        let json = peaks_to_json(&[0.0, 0.5, 1.0, 0.333333]);
        assert_eq!(json, "[0,0.5,1,0.333]");
        assert!(!json.contains('"') && !json.contains(' '));
    }
}
