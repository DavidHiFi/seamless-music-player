//! Symphonia decoder: opens a track, seeks, and hands out interleaved f32
//! frames at the file's own sample rate. The engine feeds those through the
//! resampler into the output ring. Also pulls ReplayGain tags at open so the
//! dsp stage does not have to touch the file again.

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use std::path::Path;

pub struct TrackInfo {
    pub sample_rate: u32,
    pub channels: usize,
    pub duration: f64,
    /// ReplayGain track and album gain in dB, when the file carries them.
    pub rg_track: Option<f64>,
    pub rg_album: Option<f64>,
}

pub struct Decoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    pub info: TrackInfo,
    sample_buf: Option<SampleBuffer<f32>>,
    /// Frames handed out so far since the last seek, for position sanity.
    pub frames_out: u64,
}

fn parse_gain(raw: &str) -> Option<f64> {
    let s = raw.trim().trim_end_matches("dB").trim_end_matches("db").trim();
    s.parse::<f64>().ok()
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Decoder, String> {
        let src = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("{}: {e}", path.display()))?;

        let track = probed
            .format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| format!("{}: no audio stream", path.display()))?
            .clone();
        let track_id = track.id;
        let cp = &track.codec_params;
        let sample_rate = cp.sample_rate.unwrap_or(44100);
        let channels = match &cp.channels {
            Some(c) => c.count(),
            None => 2,
        };
        let duration = cp
            .n_frames
            .map(|n| n as f64 / sample_rate as f64)
            .unwrap_or(0.0);

        let mut rg_track = None;
        let mut rg_album = None;
        let mut probe_meta = probed.metadata.get();
        if let Some(rev) = probe_meta.as_mut().and_then(|m| m.current()) {
            for tag in rev.tags() {
                let db = parse_gain(&tag.value.to_string());
                match tag.std_key {
                    Some(StandardTagKey::ReplayGainTrackGain) => rg_track = db,
                    Some(StandardTagKey::ReplayGainAlbumGain) => rg_album = db,
                    _ => {
                        // Some writers use plain keys without a standard one.
                        let k = tag.key.to_ascii_uppercase();
                        if k == "REPLAYGAIN_TRACK_GAIN" {
                            rg_track = db;
                        } else if k == "REPLAYGAIN_ALBUM_GAIN" {
                            rg_album = db;
                        } else if k == "R128_TRACK_GAIN" {
                            // R128 measures against -23 LUFS; ReplayGain
                            // against -18, so the same tag is 5 dB higher.
                            rg_track = db.map(|v| v + 5.0);
                        }
                    }
                }
            }
        }

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| format!("{}: codec {e}", path.display()))?;

        Ok(Decoder {
            format: probed.format,
            decoder,
            track_id,
            info: TrackInfo {
                sample_rate,
                channels: channels.max(1),
                duration,
                rg_track,
                rg_album,
            },
            sample_buf: None,
            frames_out: 0,
        })
    }

    /// Seek to a time in seconds. Coarse mode lands on a packet boundary,
    /// good enough for playback (Dopamine does the same).
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        let to = SeekTo::Time { time: Time::from(seconds.max(0.0)), track_id: Some(self.track_id) };
        self.format.seek(SeekMode::Coarse, to).map_err(|e| format!("seek: {e}"))?;
        self.frames_out = 0;
        Ok(())
    }

    /// Next chunk of interleaved f32 frames appended to out. Returns false
    /// at the end of the stream. Damaged packets are skipped, not fatal.
    pub fn next_chunk(&mut self, out: &mut Vec<f32>) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        || e.kind() == std::io::ErrorKind::Other =>
                {
                    return false
                }
                Err(Error::ResetRequired) => return false,
                Err(_) => return false,
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
                Err(_) => return false,
            };
            if decoded.frames() == 0 {
                continue;
            }
            let need = decoded.capacity() as usize;
            let reuse = self
                .sample_buf
                .as_ref()
                .map(|b| b.capacity() >= need)
                .unwrap_or(false);
            if !reuse {
                self.sample_buf = Some(SampleBuffer::<f32>::new(
                    decoded.capacity() as u64,
                    *decoded.spec(),
                ));
            }
            let buf = self.sample_buf.as_mut().unwrap();
            let frames = decoded.frames() as usize;
            buf.copy_interleaved_ref(decoded);
            let n = frames * self.info.channels;
            out.extend_from_slice(&buf.samples()[..n]);
            self.frames_out += n as u64 / self.info.channels as u64;
            return true;
        }
    }
}
