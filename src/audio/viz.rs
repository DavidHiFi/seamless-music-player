//! Visualizer FFT: 64 log-spaced bands from the playback stream, smoothed,
//! produced only while the UI shows the visualizer. Runs on the decode
//! thread so no extra wakeups exist anywhere.

use realfft::RealToComplex;
use std::sync::Arc;

const FFT_SIZE: usize = 1024;
pub const BARS: usize = 64;
const F_MIN: f32 = 30.0;
const F_MAX: f32 = 16000.0;
const DECAY: f32 = 0.82;

pub struct Viz {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<f32>,
    bins: Vec<(usize, usize)>,
    pub bars: [f32; BARS],
}

impl Viz {
    pub fn new(rate: u32) -> Viz {
        let mut planner = realfft::RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos()
            })
            .collect();
        // Bin ranges per band, log spaced. Bins beyond nyquist clamp to the
        // last band; the 16 kHz top is under every real device rate.
        let mut bins = Vec::with_capacity(BARS);
        let nyq = rate as f32 / 2.0;
        for i in 0..BARS {
            let f0 = F_MIN * (F_MAX / F_MIN).powf(i as f32 / BARS as f32);
            let f1 = F_MIN * (F_MAX / F_MIN).powf((i + 1) as f32 / BARS as f32);
            let b0 = ((f0 / nyq) * FFT_SIZE as f32 / 2.0) as usize;
            let b1 = (((f1 / nyq) * FFT_SIZE as f32 / 2.0) as usize).max(b0 + 1);
            bins.push((b0.min(FFT_SIZE / 2 - 1), b1.min(FFT_SIZE / 2)));
        }
        Viz {
            fft,
            window,
            input: Vec::with_capacity(FFT_SIZE),
            spectrum: Vec::new(),
            scratch: Vec::new(),
            bins,
            bars: [0.0; BARS],
        }
    }

    /// Feed mono mixdown samples; when a frame is full it is analysed and
    /// the bars update. Returns true when a new bar set is ready.
    pub fn feed(&mut self, stereo: &[f32]) -> bool {
        if self.input.len() >= FFT_SIZE {
            self.input.clear();
        }
        for pair in stereo.chunks(2) {
            self.input.push((pair[0] + pair.get(1).copied().unwrap_or(0.0)) * 0.5);
            if self.input.len() == FFT_SIZE {
                return self.analyse();
            }
        }
        false
    }

    fn analyse(&mut self) -> bool {
        let mut frame = std::mem::take(&mut self.input);
        self.input = Vec::with_capacity(FFT_SIZE);
        for (s, w) in frame.iter_mut().zip(&self.window) {
            *s *= w;
        }
        if self.fft.process(&mut frame, &mut self.spectrum).is_err() {
            return false;
        }
        for (i, &(b0, b1)) in self.bins.iter().enumerate() {
            let mut sum = 0.0f32;
            for s in &self.spectrum[b0.min(b1)..b1.max(b0 + 1).min(self.spectrum.len())] {
                sum += s.norm_sqr();
            }
            let mag = sum.sqrt() / ((b1 - b0) as f32).max(1.0);
            // Scale to a 0..1 bar: quiet music sits low, peaks touch 1.
            let db = 20.0 * mag.max(1e-6).log10();
            let v = ((db + 54.0) / 54.0).clamp(0.0, 1.0);
            self.bars[i] = v.max(self.bars[i] * DECAY);
        }
        true
    }
}
