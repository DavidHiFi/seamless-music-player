//! The dsp stage: 10-band peaking EQ, ReplayGain with preamp and a clip
//! guard, plus the gain curves the output callback uses for volume and
//! fades. Runs on the decode thread, one pass per chunk, no allocation.

/// The 10 classic bands, 31 Hz to 16 kHz.
pub const EQ_BANDS: [f64; 10] =
    [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

const EQ_Q: f64 = 1.41;

/// One peaking biquad per band per channel, direct form 1 in f64.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn peaking(freq: f64, gain_db: f64, q: f64, rate: f64) -> Biquad {
        let a = 10.0f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * freq / rate;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * c) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * c) / a0,
            a2: (1.0 - alpha / a) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

pub struct Dsp {
    rate: f64,
    eq: Vec<Biquad>,
    eq_gains: [f64; 10],
    eq_enabled: bool,
    rg_mode: String,
    preamp_db: f64,
    rg_track: Option<f64>,
    rg_album: Option<f64>,
}

impl Dsp {
    pub fn new(rate: u32) -> Dsp {
        let mut d = Dsp {
            rate: rate as f64,
            eq: Vec::new(),
            eq_gains: [0.0; 10],
            eq_enabled: false,
            rg_mode: "off".into(),
            preamp_db: 0.0,
            rg_track: None,
            rg_album: None,
        };
        d.rebuild_eq();
        d
    }

    pub fn set_eq(&mut self, enabled: bool, gains: &[f64]) {
        let mut g = [0.0f64; 10];
        for (i, v) in gains.iter().take(10).enumerate() {
            g[i] = v.clamp(-12.0, 12.0);
        }
        if g != self.eq_gains || enabled != self.eq_enabled {
            self.eq_gains = g;
            self.eq_enabled = enabled;
            self.rebuild_eq();
        }
    }

    fn rebuild_eq(&mut self) {
        self.eq = if self.eq_enabled {
            EQ_BANDS
                .iter()
                .zip(self.eq_gains)
                .flat_map(|(&f, &g)| vec![Biquad::peaking(f, g, EQ_Q, self.rate); 2])
                .collect()
        } else {
            Vec::new()
        };
    }

    pub fn set_replaygain(&mut self, mode: &str, preamp_db: f64) {
        self.rg_mode = mode.to_string();
        self.preamp_db = preamp_db.clamp(-15.0, 15.0);
    }

    /// Per-track gains from the decoder.
    pub fn set_rg_gains(&mut self, track: Option<f64>, album: Option<f64>) {
        self.rg_track = track;
        self.rg_album = album;
    }

    fn rg_gain_db(&self) -> f64 {
        let g = match self.rg_mode.as_str() {
            "track" => self.rg_track,
            "album" => self.rg_album.or(self.rg_track),
            _ => None,
        };
        g.unwrap_or(0.0).clamp(-24.0, 24.0) + self.preamp_db
    }

    /// Process one interleaved stereo chunk in place: ReplayGain, the EQ,
    /// then the clip guard.
    pub fn process(&mut self, samples: &mut [f32]) {
        let rg = 10.0f64.powf(self.rg_gain_db() / 20.0);
        let apply_rg = (rg - 1.0).abs() > 1e-4;
        for s in samples.iter_mut() {
            let mut x = *s as f64;
            if apply_rg {
                x *= rg;
            }
            for b in self.eq.chunks_mut(2) {
                x = b[0].run(x);
                x = b[1].run(x);
            }
            *s = clip_guard(x) as f32;
        }
    }
}

/// Soft guard: only touches samples near full scale so ordinary levels pass
/// through untouched.
#[inline]
fn clip_guard(x: f64) -> f64 {
    let a = x.abs();
    if a <= 0.95 {
        x
    } else {
        let over = (a - 0.95) / 0.05;
        x.signum() * (0.95 + 0.05 * over.tanh())
    }
}

/// Equal-power crossfade gains for progress t in 0..1: the outgoing track
/// fades as cos, the incoming as sin, so loudness stays even.
pub fn crossfade_gains(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let half = t * std::f64::consts::FRAC_PI_2;
    (half.cos(), half.sin())
}

/// Convert interleaved samples with `from` channels to stereo. Mono is
/// copied to both sides; more than two channels fold onto the front pair.
pub fn to_stereo(samples: &[f32], from: usize) -> Vec<f32> {
    match from {
        2 => samples.to_vec(),
        1 => samples.iter().flat_map(|&s| [s, s]).collect(),
        n if n > 2 => {
            let frames = samples.len() / n;
            let mut out = Vec::with_capacity(frames * 2);
            for f in 0..frames {
                let (mut l, mut r) = (0.0f32, 0.0f32);
                for c in 0..n {
                    let v = samples[f * n + c];
                    if c % 2 == 0 { l += v * 0.7 } else { r += v * 0.7 }
                    if c >= 2 && c < 4 {
                        // Front pair gets full weight.
                        if c == 2 { l = l - v * 0.7 + v; } else { r = r - v * 0.7 + v; }
                    }
                }
                out.push(l);
                out.push(r);
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Interleave stereo into `channels` output channels: mono averages, two
/// copies through, wider layouts keep the front pair silent elsewhere.
pub fn from_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        1 => samples.chunks(2).map(|c| (c[0] + c.get(1).copied().unwrap_or(0.0)) * 0.5).collect(),
        2 => samples.to_vec(),
        n => {
            let frames = samples.len() / 2;
            let mut out = vec![0.0f32; frames * n];
            for f in 0..frames {
                out[f * n] = samples[f * 2];
                out[f * n + 1] = samples[f * 2 + 1];
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_tone(band: usize, gain_db: f64, freq: f64) -> f64 {
        let rate = 44100.0;
        let mut d = Dsp::new(44100);
        d.set_eq(true, &{ let mut g = [0.0; 10]; g[band] = gain_db; g });
        let n = 44100; // one second settles any transient
        let mut s: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate) as f32 * 0.1)
            .collect();
        d.process(&mut s);
        // Peak of the second half, past the settle.
        s[n / 2..].iter().fold(0.0f64, |m, &v| m.max(v.abs() as f64)) * 10.0
    }

    #[test]
    fn eq_flat_is_flat() {
        let mut d = Dsp::new(44100);
        d.set_eq(true, &[0.0; 10]);
        let mut s = vec![0.25f32; 4800];
        d.process(&mut s);
        for v in s {
            assert!((v as f64 - 0.25).abs() < 1e-4, "flat EQ changed the level");
        }
    }

    #[test]
    fn eq_band_boosts() {
        // +12 dB at 1 kHz should land near x4 (12 dB) on a 1 kHz tone.
        let peak = run_tone(5, 12.0, 1000.0);
        assert!((peak - 4.0).abs() < 0.5, "1k boost gave {peak}");
    }

    #[test]
    fn eq_band_neighbours_stay_low() {
        // A 1 kHz tone through a +12 dB boost at 31 Hz barely moves.
        let peak = run_tone(0, 12.0, 1000.0);
        assert!(peak < 1.3, "31 Hz band leaked into 1 kHz: {peak}");
    }

    #[test]
    fn crossfade_is_equal_power() {
        let mut worst = 0.0f64;
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let (o, n) = crossfade_gains(t);
            worst = worst.max((o * o + n * n - 1.0).abs());
        }
        assert!(worst < 1e-6, "crossfade power {worst}");
        let (o, n) = crossfade_gains(0.5);
        assert!((o - 0.7071).abs() < 1e-3 && (n - 0.7071).abs() < 1e-3);
    }

    #[test]
    fn stereo_conversions() {
        let mono = vec![0.5f32];
        assert_eq!(to_stereo(&mono, 1), vec![0.5, 0.5]);
        let st = vec![0.2f32, 0.6];
        assert_eq!(from_stereo(&st, 1), vec![0.4]);
        let wide = from_stereo(&st, 6);
        assert_eq!(wide.len(), 6);
        assert_eq!(wide[0], 0.2);
        assert_eq!(wide[5], 0.0);
    }

    #[test]
    fn clip_guard_kicks_in_only_at_the_top() {
        assert_eq!(clip_guard(0.5), 0.5);
        let g = clip_guard(1.4);
        assert!(g > 0.99 && g < 1.0, "guard gave {g}");
    }
}
