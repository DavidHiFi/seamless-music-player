//! rubato resampler: file rate to the output device rate, FFT based so the
//! CPU cost stays tiny. rubato's fixed resampler only takes exact-size
//! blocks, while decoders hand out packets of any size, so incoming audio is
//! queued here and converted one full block at a time. Matching rates skip
//! rubato entirely.

use rubato::{FftFixedInOut, Resampler as RubatoResampler};

pub struct Resampler {
    /// None when the file and device rates match: audio passes straight on.
    r: Option<FftFixedInOut<f32>>,
    from_rate: u32,
    channels: usize,
    /// Interleaved frames waiting for a full block.
    queue: Vec<f32>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32, channels: usize) -> Result<Resampler, String> {
        let channels = channels.max(1);
        if from_rate == to_rate {
            return Ok(Resampler { r: None, from_rate, channels, queue: Vec::new(), input: Vec::new(), output: Vec::new() });
        }
        let r = FftFixedInOut::new(from_rate as usize, to_rate as usize, 1024, channels)
            .map_err(|e| format!("resampler: {e}"))?;
        let input = vec![vec![0.0; r.input_frames_max()]; channels];
        let output = vec![vec![0.0; r.output_frames_max()]; channels];
        Ok(Resampler { r: Some(r), from_rate, channels, queue: Vec::new(), input, output })
    }

    pub fn from_rate(&self) -> u32 {
        self.from_rate
    }

    pub fn input_chunk(&self) -> usize {
        self.r.as_ref().map(|r| r.input_frames_next()).unwrap_or(1024)
    }

    /// Drop queued samples and clear filter state (used after a seek).
    pub fn reset(&mut self) {
        self.queue.clear();
        if let Some(r) = self.r.as_mut() {
            r.reset();
        }
    }

    /// Resample interleaved audio of any length. Returns every full block
    /// converted so far; the remainder waits for the next call.
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let ch = self.channels;
        let r = match self.r.as_mut() {
            None => return interleaved.to_vec(),
            Some(r) => r,
        };
        self.queue.extend_from_slice(interleaved);
        let mut out = Vec::new();
        let mut used = 0;
        loop {
            let need = r.input_frames_next();
            if (self.queue.len() - used) / ch < need {
                break;
            }
            for (c, buf) in self.input.iter_mut().enumerate() {
                buf.clear();
                buf.extend((0..need).map(|f| self.queue[used + f * ch + c]));
            }
            used += need * ch;
            match r.process_into_buffer(&self.input, &mut self.output, None) {
                Ok((_, written)) => push_interleaved(&mut out, &self.output, written, ch),
                Err(e) => eprintln!("resample: {e}"),
            }
        }
        self.queue.drain(..used);
        out
    }

    /// Flush whatever is queued at the end of a track (plus an optional tail).
    pub fn flush(&mut self, tail: &[f32]) -> Vec<f32> {
        let mut out = self.process(tail);
        let ch = self.channels;
        let r = match self.r.as_mut() {
            None => return out,
            Some(r) => r,
        };
        if self.queue.is_empty() {
            return out;
        }
        let frames = self.queue.len() / ch;
        for (c, buf) in self.input.iter_mut().enumerate() {
            buf.clear();
            buf.extend((0..frames).map(|f| self.queue[f * ch + c]));
        }
        self.queue.clear();
        if let Ok(o) = r.process_partial(Some(&self.input), None) {
            let n = o.first().map(|c| c.len()).unwrap_or(0);
            push_interleaved(&mut out, &o, n, ch);
        }
        out
    }
}

fn push_interleaved(out: &mut Vec<f32>, buf: &[Vec<f32>], frames: usize, channels: usize) {
    let frames = buf.iter().map(|c| c.len()).min().unwrap_or(0).min(frames);
    out.reserve(frames * channels);
    for f in 0..frames {
        for c in 0..channels {
            out.push(buf[c][f]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_tone(from_rate: u32, to_rate: u32, freq: f64, chunks: &[usize]) -> (usize, f64) {
        let mut r = Resampler::new(from_rate, to_rate, 2).expect("resampler");
        let total_in = from_rate as usize; // 1.0 second
        let mut src = Vec::with_capacity(total_in * 2);
        for i in 0..total_in {
            let v = (2.0 * std::f64::consts::PI * freq * i as f64 / from_rate as f64).sin() as f32;
            src.push(v);
            src.push(v);
        }
        let mut out = Vec::new();
        let mut pos = 0;
        let mut ci = 0;
        while pos < total_in {
            let n = chunks[ci % chunks.len()].min(total_in - pos);
            ci += 1;
            out.extend_from_slice(&r.process(&src[pos * 2..(pos + n) * 2]));
            pos += n;
        }
        out.extend_from_slice(&r.flush(&[]));
        let frames = out.len() / 2;
        // Count rising zero-crossings on left channel across the steady middle half.
        let a = frames / 4;
        let b = (frames * 3) / 4;
        let mut crossings = 0usize;
        for f in a..b {
            if out[(f - 1) * 2] <= 0.0 && out[f * 2] > 0.0 {
                crossings += 1;
            }
        }
        let measured = crossings as f64 * to_rate as f64 / (b - a) as f64;
        (frames, measured)
    }

    #[test]
    fn passthrough_when_rates_match() {
        let mut r = Resampler::new(44100, 44100, 2).unwrap();
        let inp = vec![0.1, -0.2, 0.3, -0.4];
        assert_eq!(r.process(&inp), inp);
        assert!(r.flush(&[]).is_empty());
    }

    #[test]
    fn resample_44100_to_48000_keeps_pitch_and_length() {
        let (frames, freq) = measure_tone(44100, 48000, 440.0, &[1152, 576, 4096, 300, 1024]);
        assert!((frames as i64 - 48000).abs() < 1200, "frames={frames}");
        assert!((freq - 440.0).abs() < 2.0, "pitch drifted to {freq} Hz");
    }

    #[test]
    fn resample_48000_to_44100_keeps_pitch_and_length() {
        let (frames, freq) = measure_tone(48000, 44100, 440.0, &[960, 1024, 4096, 512]);
        assert!((frames as i64 - 44100).abs() < 1200, "frames={frames}");
        assert!((freq - 440.0).abs() < 2.0, "pitch drifted to {freq} Hz");
    }

    #[test]
    fn resample_44100_to_96000_keeps_pitch_and_length() {
        let (frames, freq) = measure_tone(44100, 96000, 440.0, &[1152, 4096]);
        assert!((frames as i64 - 96000).abs() < 2400, "frames={frames}");
        assert!((freq - 440.0).abs() < 2.0, "pitch drifted to {freq} Hz");
    }
}
