//! rubato resampler: file rate to the output device rate, FFT based so the
//! CPU cost stays tiny. The engine feeds fixed stereo chunks and gets the
//! device-rate chunk back.

use rubato::{FftFixedInOut, Resampler as RubatoResampler};

pub struct Resampler {
    r: FftFixedInOut<f32>,
    channels: usize,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32, channels: usize) -> Result<Resampler, String> {
        if from_rate == to_rate {
            // rubato still needs a resampler object for uniform handling;
            // a 1:1 FftFixedInOut is a pass-through with a tiny delay.
        }
        let r = FftFixedInOut::new(from_rate as usize, to_rate as usize, 2048, channels)
            .map_err(|e| format!("resampler: {e}"))?;
        let input = vec![Vec::new(); channels];
        let output = vec![vec![0.0; r.output_frames_max()]; channels];
        Ok(Resampler { r, channels, input, output })
    }

    pub fn input_chunk(&self) -> usize {
        self.r.input_frames_next()
    }

    /// Resample one interleaved stereo chunk; returns interleaved output.
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let frames = interleaved.len() / self.channels;
        for (c, ch) in self.input.iter_mut().enumerate() {
            ch.clear();
            ch.reserve(frames);
            for f in 0..frames {
                ch.push(interleaved[f * self.channels + c]);
            }
        }
        match self.r.process_into_buffer(&self.input, &mut self.output, None) {
            Ok((_, written)) => {
                let n = written.min(self.output[0].len());
                interleave(&self.output, n, self.channels)
            }
            Err(e) => {
                // Never drop audio silently: return the chunk unchanged so a
                // bad rate pair still plays (device and file rates match in
                // that case anyway).
                eprintln!("resample: {e}");
                interleaved.to_vec()
            }
        }
    }

    /// Flush whatever is left at the end of a track.
    pub fn flush(&mut self, tail: &[f32]) -> Vec<f32> {
        let frames = tail.len() / self.channels;
        for (c, ch) in self.input.iter_mut().enumerate() {
            ch.clear();
            for f in 0..frames {
                ch.push(tail[f * self.channels + c]);
            }
        }
        match self.r.process_partial(Some(&self.input), None) {
            Ok(out) => {
                let n = out.first().map(|c| c.len()).unwrap_or(0);
                interleave(&out, n, self.channels)
            }
            Err(_) => Vec::new(),
        }
    }
}

fn interleave(buf: &[Vec<f32>], frames: usize, channels: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * channels);
    for f in 0..frames {
        for c in 0..channels {
            out.push(buf[c][f]);
        }
    }
    out
}
