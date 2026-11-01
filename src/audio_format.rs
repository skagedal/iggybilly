//! What an uploaded file actually is, judged by its bytes, and making a
//! playable copy of the formats some client can't play as stored.
//!
//! See `specs/drafts/audio-formats.md`. The extension and the uploader's
//! Content-Type play no part: phones misname files, and the served type
//! has to match the bytes or Safari and AVPlayer refuse them.

use std::fmt;
use std::io::{BufWriter, Write};
use std::path::Path;

use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// How much of the file `sniff` looks at. Enough to get past an ID3 tag
/// with a small cover image; a larger tag just means a refusal.
pub const SNIFF_BYTES: usize = 64 * 1024;

/// A format we accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp4Aac,
    Mp3,
    Adts,
}

impl AudioFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            AudioFormat::Mp4Aac => "audio/mp4",
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Adts => "audio/aac",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Mp4Aac => "m4a",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Adts => "aac",
        }
    }

    /// The value stored in `clips.format`.
    pub fn as_str(self) -> &'static str {
        match self {
            AudioFormat::Mp4Aac => "mp4-aac",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Adts => "adts",
        }
    }

    /// Whether every client can play the file as stored. The ones that
    /// can't get an MP4 playback copy.
    pub fn needs_playback_copy(self) -> bool {
        self == AudioFormat::Adts
    }
}

/// A file we refuse, named as precisely as its bytes allow so the
/// message can say what it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    OggVorbis,
    OggOpus,
    Ogg,
    WebM,
    Wav,
    Flac,
    Amr,
    Mp4Alac,
    Mp4NoAac,
    Unknown,
}

impl Unsupported {
    fn description(self) -> Option<&'static str> {
        Some(match self {
            Unsupported::OggVorbis => "Ogg Vorbis",
            Unsupported::OggOpus => "Ogg Opus",
            Unsupported::Ogg => "Ogg",
            Unsupported::WebM => "WebM",
            Unsupported::Wav => "WAV",
            Unsupported::Flac => "FLAC",
            Unsupported::Amr => "AMR",
            Unsupported::Mp4Alac => "MP4 with Apple Lossless (ALAC) audio",
            Unsupported::Mp4NoAac => "MP4 without AAC audio",
            Unsupported::Unknown => return None,
        })
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.description() {
            Some(d) => write!(
                f,
                "This file is {d}, which we don't support at the moment. \
                 Talk to Simon if you want to change this."
            )?,
            None => write!(f, "This isn't an audio file we recognise.")?,
        }
        write!(f, " Upload MP3, M4A or AAC.")
    }
}

/// What the first bytes say, before the MP4 codec check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sniffed {
    Mp4,
    Accepted(AudioFormat),
    Refused(Unsupported),
}

/// Identify the file at `path` by its content.
pub fn detect(path: &Path) -> std::io::Result<Result<AudioFormat, Unsupported>> {
    let head = read_head(path)?;
    Ok(match sniff(&head) {
        Sniffed::Accepted(f) => Ok(f),
        Sniffed::Refused(u) => Err(u),
        Sniffed::Mp4 => mp4_codec(path),
    })
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    std::fs::File::open(path)?
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// The track list is needed to tell AAC from ALAC, and `moov` may be at
/// the end of the file, so this reads the container with symphonia, as
/// the waveform pass does.
fn mp4_codec(path: &Path) -> Result<AudioFormat, Unsupported> {
    let Ok(file) = std::fs::File::open(path) else {
        return Err(Unsupported::Unknown);
    };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let Ok(format) = symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) else {
        return Err(Unsupported::Mp4NoAac);
    };
    let codec = format
        .default_track(TrackType::Audio)
        .and_then(|t| t.codec_params.as_ref())
        .and_then(|p| p.audio())
        .map(|a| a.codec);
    match codec {
        Some(c) if c == CODEC_ID_AAC => Ok(AudioFormat::Mp4Aac),
        Some(c) if c == symphonia::core::codecs::audio::well_known::CODEC_ID_ALAC => {
            Err(Unsupported::Mp4Alac)
        }
        _ => Err(Unsupported::Mp4NoAac),
    }
}

fn sniff(head: &[u8]) -> Sniffed {
    if head.len() >= 8 && &head[4..8] == b"ftyp" {
        return Sniffed::Mp4;
    }
    if head.starts_with(b"OggS") {
        return Sniffed::Refused(ogg_kind(head));
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Sniffed::Refused(Unsupported::WebM);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Sniffed::Refused(Unsupported::Wav);
    }
    if head.starts_with(b"fLaC") {
        return Sniffed::Refused(Unsupported::Flac);
    }
    if head.starts_with(b"#!AMR") {
        return Sniffed::Refused(Unsupported::Amr);
    }

    // MP3 and ADTS can both start with an ID3 tag, and their frame
    // headers look alike, so look past the tag and require two frames in
    // a row before believing either.
    let start = id3_len(head).unwrap_or(0);
    let Some(body) = head.get(start..) else {
        return Sniffed::Refused(Unsupported::Unknown);
    };
    if body.starts_with(b"fLaC") {
        return Sniffed::Refused(Unsupported::Flac);
    }
    if adts_frames_follow(body) {
        return Sniffed::Accepted(AudioFormat::Adts);
    }
    if mp3_frames_follow(body) {
        return Sniffed::Accepted(AudioFormat::Mp3);
    }
    Sniffed::Refused(Unsupported::Unknown)
}

/// The first Ogg page carries the codec's identification header.
fn ogg_kind(head: &[u8]) -> Unsupported {
    let first_packet = head.get(28..).unwrap_or(&[]);
    if first_packet.starts_with(b"OpusHead") {
        Unsupported::OggOpus
    } else if first_packet.starts_with(b"\x01vorbis") {
        Unsupported::OggVorbis
    } else {
        Unsupported::Ogg
    }
}

/// Length of a leading ID3v2 tag, header and optional footer included.
fn id3_len(b: &[u8]) -> Option<usize> {
    if b.len() < 10 || &b[0..3] != b"ID3" {
        return None;
    }
    let size = b[6..10]
        .iter()
        .fold(0usize, |acc, &x| (acc << 7) | (x & 0x7F) as usize);
    let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
    Some(10 + size + footer)
}

// -- ADTS -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdtsHeader {
    /// MPEG-4 audio object type minus one, as ADTS stores it.
    profile: u8,
    freq_index: u8,
    channels: u8,
    header_len: usize,
    frame_len: usize,
    raw_blocks: u8,
}

fn adts_header(b: &[u8]) -> Option<AdtsHeader> {
    if b.len() < 7 || b[0] != 0xFF || b[1] & 0xF6 != 0xF0 {
        return None;
    }
    let protection_absent = b[1] & 0x01 != 0;
    let freq_index = (b[2] >> 2) & 0x0F;
    if freq_index > 12 {
        return None;
    }
    let header_len = if protection_absent { 7 } else { 9 };
    let frame_len =
        (((b[3] & 0x03) as usize) << 11) | ((b[4] as usize) << 3) | ((b[5] >> 5) as usize);
    if frame_len < header_len {
        return None;
    }
    Some(AdtsHeader {
        profile: b[2] >> 6,
        freq_index,
        channels: ((b[2] & 0x01) << 2) | (b[3] >> 6),
        header_len,
        frame_len,
        raw_blocks: b[6] & 0x03,
    })
}

fn adts_frames_follow(b: &[u8]) -> bool {
    let Some(first) = adts_header(b) else {
        return false;
    };
    match b.get(first.frame_len..) {
        // A single-frame file is too short to matter; accept it.
        Some([]) => true,
        Some(rest) => adts_header(rest).is_some(),
        // The second header is past what was sniffed.
        None => b.len() == SNIFF_BYTES,
    }
}

// -- MP3 ------------------------------------------------------------------

/// Length in bytes of the MPEG-1/2/2.5 Layer III frame starting at `b`.
fn mp3_frame_len(b: &[u8]) -> Option<usize> {
    if b.len() < 4 || b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (b[1] >> 3) & 0x03; // 00 = 2.5, 10 = 2, 11 = 1
    let layer = (b[1] >> 1) & 0x03; // 01 = Layer III
    if version == 0b01 || layer != 0b01 {
        return None;
    }
    let bitrate_index = (b[2] >> 4) as usize;
    let rate_index = ((b[2] >> 2) & 0x03) as usize;
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }
    const BITRATES_V1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const BITRATES_V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const RATES_V1: [u32; 3] = [44100, 48000, 32000];
    let (kbps, rate, factor) = match version {
        0b11 => (BITRATES_V1[bitrate_index], RATES_V1[rate_index], 144),
        0b10 => (BITRATES_V2[bitrate_index], RATES_V1[rate_index] / 2, 72),
        _ => (BITRATES_V2[bitrate_index], RATES_V1[rate_index] / 4, 72),
    };
    let padding = ((b[2] >> 1) & 0x01) as u32;
    Some((factor * kbps * 1000 / rate + padding) as usize)
}

fn mp3_frames_follow(b: &[u8]) -> bool {
    let Some(len) = mp3_frame_len(b) else {
        return false;
    };
    match b.get(len..) {
        Some([]) => true,
        Some(rest) => mp3_frame_len(rest).is_some(),
        None => b.len() == SNIFF_BYTES,
    }
}

// -- Remux ----------------------------------------------------------------

/// Write the AAC frames of the ADTS file at `src` into an MP4 at `dst`.
/// The audio is copied, not re-encoded. The index (`moov`) goes before
/// the audio, so a player knows the duration and can seek after one
/// request.
pub fn remux_adts_to_mp4(src: &Path, dst: &Path) -> anyhow::Result<()> {
    use anyhow::{bail, Context};

    let data = std::fs::read(src).context("reading ADTS file")?;
    let mut pos = id3_len(&data).unwrap_or(0);

    let mut frames: Vec<&[u8]> = Vec::new();
    let mut first: Option<AdtsHeader> = None;
    while pos < data.len() {
        let Some(h) = adts_header(&data[pos..]) else {
            bail!("no ADTS frame header at byte {pos}");
        };
        if pos + h.frame_len > data.len() {
            // A recording cut off mid-frame: drop the partial frame.
            break;
        }
        if h.raw_blocks != 0 {
            bail!("ADTS frame with several raw data blocks at byte {pos}");
        }
        match first {
            None => first = Some(h),
            Some(f)
                if (f.profile, f.freq_index, f.channels)
                    != (h.profile, h.freq_index, h.channels) =>
            {
                bail!("ADTS stream changes format at byte {pos}");
            }
            _ => {}
        }
        frames.push(&data[pos + h.header_len..pos + h.frame_len]);
        pos += h.frame_len;
    }
    let Some(first) = first else {
        bail!("no ADTS frames");
    };
    if first.channels == 0 {
        bail!("ADTS channel layout given in-band, which MP4 can't carry this way");
    }

    let track = AacTrack {
        object_type: first.profile + 1,
        freq_index: first.freq_index,
        channels: first.channels,
        frame_sizes: frames.iter().map(|f| f.len() as u32).collect(),
    };
    let ftyp = mp4_box(
        b"ftyp",
        &[b"M4A ", &0u32.to_be_bytes()[..], b"M4A ", b"mp42", b"isom"].concat(),
    );
    // The chunk offset depends on the size of moov, which doesn't depend
    // on the offset's value: build it once to measure, then for real.
    let moov_len = track.moov(0).len();
    let audio_offset = (ftyp.len() + moov_len + 8) as u32;
    let moov = track.moov(audio_offset);

    let payload_len: usize = frames.iter().map(|f| f.len()).sum();
    let mut out = BufWriter::new(std::fs::File::create(dst).context("creating MP4 file")?);
    out.write_all(&ftyp)?;
    out.write_all(&moov)?;
    out.write_all(&((payload_len + 8) as u32).to_be_bytes())?;
    out.write_all(b"mdat")?;
    for f in &frames {
        out.write_all(f)?;
    }
    out.flush()?;
    Ok(())
}

const AAC_FRAME_SAMPLES: u32 = 1024;
const SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

/// One AAC track in a single chunk, everything an M4A's `moov` needs.
struct AacTrack {
    object_type: u8,
    freq_index: u8,
    channels: u8,
    frame_sizes: Vec<u32>,
}

impl AacTrack {
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATES[self.freq_index as usize]
    }

    fn duration(&self) -> u32 {
        self.frame_sizes.len() as u32 * AAC_FRAME_SAMPLES
    }

    fn moov(&self, audio_offset: u32) -> Vec<u8> {
        let rate = self.sample_rate();
        let duration = self.duration();
        let matrix: Vec<u8> = [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();

        let mvhd = full_box(
            b"mvhd",
            0,
            &[
                &0u32.to_be_bytes()[..], // creation
                &0u32.to_be_bytes(),     // modification
                &rate.to_be_bytes(),
                &duration.to_be_bytes(),
                &0x0001_0000u32.to_be_bytes(), // rate 1.0
                &0x0100u16.to_be_bytes(),      // volume 1.0
                &[0; 10],
                &matrix,
                &[0; 24],
                &2u32.to_be_bytes(), // next track id
            ]
            .concat(),
        );
        let tkhd = full_box(
            b"tkhd",
            0x000003, // enabled, in movie
            &[
                &0u32.to_be_bytes()[..],
                &0u32.to_be_bytes(),
                &1u32.to_be_bytes(), // track id
                &[0; 4],
                &duration.to_be_bytes(),
                &[0; 8],
                &0u16.to_be_bytes(),      // layer
                &0u16.to_be_bytes(),      // alternate group
                &0x0100u16.to_be_bytes(), // volume
                &[0; 2],
                &matrix,
                &0u32.to_be_bytes(), // width
                &0u32.to_be_bytes(), // height
            ]
            .concat(),
        );
        let mdhd = full_box(
            b"mdhd",
            0,
            &[
                &0u32.to_be_bytes()[..],
                &0u32.to_be_bytes(),
                &rate.to_be_bytes(),
                &duration.to_be_bytes(),
                &0x55C4u16.to_be_bytes(), // "und"
                &[0; 2],
            ]
            .concat(),
        );
        let hdlr = full_box(
            b"hdlr",
            0,
            &[&[0; 4][..], b"soun", &[0; 12], b"SoundHandler\0"].concat(),
        );
        let smhd = full_box(b"smhd", 0, &[0; 4]);
        let dinf = mp4_box(
            b"dinf",
            &full_box(
                b"dref",
                0,
                &[&1u32.to_be_bytes()[..], &full_box(b"url ", 1, &[])].concat(),
            ),
        );
        let stbl = mp4_box(
            b"stbl",
            &[
                full_box(
                    b"stsd",
                    0,
                    &[&1u32.to_be_bytes()[..], &self.mp4a()].concat(),
                ),
                full_box(
                    b"stts",
                    0,
                    &[
                        &1u32.to_be_bytes()[..],
                        &(self.frame_sizes.len() as u32).to_be_bytes(),
                        &AAC_FRAME_SAMPLES.to_be_bytes(),
                    ]
                    .concat(),
                ),
                full_box(
                    b"stsc",
                    0,
                    &[
                        &1u32.to_be_bytes()[..],
                        &1u32.to_be_bytes(), // first chunk
                        &(self.frame_sizes.len() as u32).to_be_bytes(),
                        &1u32.to_be_bytes(), // sample description
                    ]
                    .concat(),
                ),
                full_box(
                    b"stsz",
                    0,
                    &[
                        &0u32.to_be_bytes()[..],
                        &(self.frame_sizes.len() as u32).to_be_bytes(),
                        &self
                            .frame_sizes
                            .iter()
                            .flat_map(|s| s.to_be_bytes())
                            .collect::<Vec<u8>>(),
                    ]
                    .concat(),
                ),
                full_box(
                    b"stco",
                    0,
                    &[&1u32.to_be_bytes()[..], &audio_offset.to_be_bytes()].concat(),
                ),
            ]
            .concat(),
        );
        let minf = mp4_box(b"minf", &[smhd, dinf, stbl].concat());
        let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
        mp4_box(b"moov", &[mvhd, trak].concat())
    }

    fn mp4a(&self) -> Vec<u8> {
        // 16.16 fixed point; rates above 65535 don't fit and the decoder
        // takes the rate from the esds anyway.
        let rate_field = if self.sample_rate() <= 0xFFFF {
            self.sample_rate() << 16
        } else {
            0
        };
        mp4_box(
            b"mp4a",
            &[
                &[0; 6][..],
                &1u16.to_be_bytes(), // data reference index
                &[0; 8],
                &(self.channels as u16).to_be_bytes(),
                &16u16.to_be_bytes(), // sample size
                &[0; 4],
                &rate_field.to_be_bytes(),
                &self.esds(),
            ]
            .concat(),
        )
    }

    fn esds(&self) -> Vec<u8> {
        // AudioSpecificConfig: object type (5 bits), frequency index (4),
        // channel configuration (4), three zero flag bits.
        let asc = (((self.object_type as u16) << 11)
            | ((self.freq_index as u16) << 7)
            | ((self.channels as u16) << 3))
            .to_be_bytes();
        let total: u64 = self.frame_sizes.iter().map(|&s| s as u64).sum();
        let samples = self.duration() as u64;
        let avg_bitrate = (total * 8 * self.sample_rate() as u64 / samples.max(1)) as u32;
        let max_frame = self.frame_sizes.iter().copied().max().unwrap_or(0);
        let max_bitrate =
            (max_frame as u64 * 8 * self.sample_rate() as u64 / AAC_FRAME_SAMPLES as u64) as u32;

        let decoder_config = descriptor(
            0x04,
            &[
                &[0x40, 0x15][..],             // MPEG-4 audio; audio stream
                &max_frame.to_be_bytes()[1..], // buffer size, 24 bits
                &max_bitrate.to_be_bytes(),
                &avg_bitrate.to_be_bytes(),
                &descriptor(0x05, &asc),
            ]
            .concat(),
        );
        let es = descriptor(
            0x03,
            &[
                &0u16.to_be_bytes()[..], // ES id
                &[0],                    // flags
                &decoder_config,
                &descriptor(0x06, &[0x02]), // SL config: predefined for MP4
            ]
            .concat(),
        );
        full_box(b"esds", 0, &es)
    }
}

fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat()
}

fn full_box(kind: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    // Version 0 in the top byte, flags in the rest.
    mp4_box(
        kind,
        &[&(flags & 0x00FF_FFFF).to_be_bytes()[..], body].concat(),
    )
}

/// An MPEG-4 descriptor with a single-byte length, which every one here
/// fits in.
fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    debug_assert!(body.len() < 0x80);
    [&[tag, body.len() as u8][..], body].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio")
            .join(name)
    }

    #[test]
    fn detects_each_fixture() {
        let cases = [
            ("../guitar-clip.m4a", Ok(AudioFormat::Mp4Aac)),
            ("clip.mp3", Ok(AudioFormat::Mp3)),
            ("clip-bare.mp3", Ok(AudioFormat::Mp3)),
            ("clip.aac", Ok(AudioFormat::Adts)),
            ("clip-alac.m4a", Err(Unsupported::Mp4Alac)),
            ("clip.ogg", Err(Unsupported::OggOpus)),
            ("clip.webm", Err(Unsupported::WebM)),
            ("clip.wav", Err(Unsupported::Wav)),
            ("clip.flac", Err(Unsupported::Flac)),
        ];
        for (name, want) in cases {
            assert_eq!(detect(&fixture(name)).unwrap(), want, "{name}");
        }
    }

    #[test]
    fn adts_behind_an_id3_tag_is_still_adts() {
        let adts = std::fs::read(fixture("clip.aac")).unwrap();
        let mut tagged = vec![b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0];
        tagged.extend_from_slice(&adts);
        assert_eq!(sniff(&tagged), Sniffed::Accepted(AudioFormat::Adts));
    }

    #[test]
    fn text_is_not_audio() {
        assert_eq!(
            sniff(b"<html><body>hello</body></html>"),
            Sniffed::Refused(Unsupported::Unknown)
        );
        assert_eq!(sniff(b""), Sniffed::Refused(Unsupported::Unknown));
    }

    #[test]
    fn a_lone_sync_word_is_not_audio() {
        let mut b = vec![0xFF, 0xF1, 0x50, 0x80, 0x02, 0x1F, 0xFC];
        b.extend_from_slice(&[0u8; 64]);
        assert_eq!(sniff(&b), Sniffed::Refused(Unsupported::Unknown));
    }

    #[test]
    fn refusal_message_names_the_format() {
        assert_eq!(
            Unsupported::OggVorbis.to_string(),
            "This file is Ogg Vorbis, which we don't support at the moment. \
             Talk to Simon if you want to change this. Upload MP3, M4A or AAC."
        );
        assert_eq!(
            Unsupported::Unknown.to_string(),
            "This isn't an audio file we recognise. Upload MP3, M4A or AAC."
        );
    }

    #[test]
    fn remuxed_adts_is_mp4_aac_with_the_same_duration() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.m4a");
        remux_adts_to_mp4(&fixture("clip.aac"), &out).unwrap();

        assert_eq!(detect(&out).unwrap(), Ok(AudioFormat::Mp4Aac));
        let original = crate::audio::compute(&fixture("clip.aac")).unwrap();
        let remuxed = crate::audio::compute(&out).unwrap();
        assert!(
            (original.duration_seconds - remuxed.duration_seconds).abs() < 0.05,
            "{} vs {}",
            original.duration_seconds,
            remuxed.duration_seconds
        );
    }
}
