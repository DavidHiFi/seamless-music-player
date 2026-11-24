//! cpal output: WASAPI shared mode on the chosen device, f32, with the
//! volume ramp and the short play/pause/seek fades applied right in the
//! device callback so even buffered audio fades cleanly. No allocation in
//! the callback: the ring, the scratch view and the gain state are all
//! fixed when the stream opens.

use crate::audio::ring::Ring;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Gain state shared between the player (writers) and the device callback.
pub struct Shared {
    pub ring: RwLock<Arc<Ring>>,
    pub volume: AtomicU32,     // f32 bits, 0..1
    pub fade_target: AtomicU32, // f32 bits, 0 or 1
    pub fade_cur: AtomicU32,   // f32 bits, ramps toward the target
    pub fade_step: AtomicU32,  // f32 bits, per-frame gain change
    pub consumed: AtomicU64,   // frames the device has read since the swap
    pub rate: AtomicU32,
    pub channels: AtomicU32,
    pub alive: AtomicBool,
}

impl Shared {
    pub fn new(rate: u32, channels: u32, ring_frames: usize) -> Arc<Shared> {
        let fade_step = (0.004 / (rate as f32 * 0.005)).max(0.0002);
        Arc::new(Shared {
            ring: RwLock::new(Arc::new(Ring::new(ring_frames, channels as usize))),
            volume: AtomicU32::new(1.0f32.to_bits()),
            fade_target: AtomicU32::new(0.0f32.to_bits()),
            fade_cur: AtomicU32::new(0.0f32.to_bits()),
            fade_step: AtomicU32::new(fade_step.to_bits()),
            consumed: AtomicU64::new(0),
            rate: AtomicU32::new(rate),
            channels: AtomicU32::new(channels),
            alive: AtomicBool::new(true),
        })
    }

    /// Swap in a fresh ring and reset the consumed counter as one step, so
    /// the callback never sees a half-updated position.
    pub fn swap_ring(&self, ring: Arc<Ring>) {
        let mut g = self.ring.write().unwrap();
        self.consumed.store(0, Ordering::Release);
        *g = ring;
    }

    fn f32a(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Ordering::Relaxed))
    }

    /// Frames the device has played since the last swap.
    pub fn played_frames(&self) -> u64 {
        self.consumed.load(Ordering::Relaxed)
    }
}

pub struct Output {
    #[allow(dead_code)]
    stream: cpal::Stream,
    pub shared: Arc<Shared>,
    pub device_name: String,
    pub rate: u32,
    pub channels: u32,
}

/// Output devices cpal can see, best first: (id = name, is_default).
pub fn list_devices() -> Vec<(String, bool)> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            if let Ok(name) = d.name() {
                let is_default = Some(&name) == default.as_ref();
                out.push((name, is_default));
            }
        }
    }
    out
}

pub fn open(preferred: Option<&str>) -> Result<Output, String> {
    let host = cpal::default_host();
    let mut last_err = String::from("no output device");
    let mut try_device = |device: cpal::Device, name: String| -> Result<Output, String> {
        let cfg = device
            .default_output_config()
            .map_err(|e| format!("{name}: {e}"))?;
        let channels = cfg.channels();
        let rate = cfg.sample_rate().0;
        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(rate),
            buffer_size: cpal::BufferSize::Default,
        };
        // About 200 ms of cushion; the ring is a power of two.
        let ring_frames = (rate as usize * channels as usize / 4).next_power_of_two();
        let shared = Shared::new(rate, channels as u32, ring_frames);
        let cb_shared = Arc::clone(&shared);
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill(&cb_shared, data, &mut scratch);
                },
                {
                    let s = Arc::clone(&shared);
                    move |_e| s.alive.store(false, Ordering::Relaxed)
                },
                None,
            )
            .map_err(|e| format!("{name}: {e}"))?;
        stream.play().map_err(|e| format!("{name}: play {e}"))?;
        Ok(Output { stream, shared, device_name: name, rate, channels: channels as u32 })
    };

    if let Some(want) = preferred {
        if let Ok(devs) = host.output_devices() {
            for d in devs {
                if let Ok(name) = d.name() {
                    if name == want {
                        match try_device(d, name) {
                            Ok(o) => return Ok(o),
                            Err(e) => last_err = e,
                        }
                        break;
                    }
                }
            }
        }
    }
    // Preferred device missing or failed: the default one.
    if let Some(d) = host.default_output_device() {
        let name = d.name().unwrap_or_else(|_| "default".into());
        return try_device(d, name).map_err(|e| { let _ = &mut last_err; e });
    }
    Err(last_err)
}

/// The device callback: read the ring, zero what is missing, apply the
/// ramped fade and volume. One ring read lock per callback, nothing else.
fn fill(shared: &Shared, data: &mut [f32], _scratch: &mut Vec<f32>) {
    if !shared.alive.load(Ordering::Relaxed) {
        data.fill(0.0);
        return;
    }
    let ch = shared.channels.load(Ordering::Relaxed).max(1) as usize;
    let ring = shared.ring.read().unwrap().clone();
    let n = ring.read(data);
    for s in data[n * ch..].iter_mut() {
        *s = 0.0;
    }
    // Gain: fade ramps toward its target, volume is direct. Both multiply.
    let step = Shared::f32a(&shared.fade_step);
    let target = Shared::f32a(&shared.fade_target);
    let mut cur = Shared::f32a(&shared.fade_cur);
    cur += (target - cur).clamp(-step, step);
    shared.fade_cur.store(cur.to_bits(), Ordering::Relaxed);
    let vol = Shared::f32a(&shared.volume);
    let g = cur * vol;
    if g < 0.999 {
        for s in data[..n * ch].iter_mut() {
            *s *= g;
        }
    }
    shared.consumed.fetch_add(n as u64, Ordering::Relaxed);
}

/// Watch for a device that went away: returns true when the stream errored.
pub fn is_dead(shared: &Shared) -> bool {
    !shared.alive.load(Ordering::Relaxed)
}

/// Keep the alive flag in a box the engine can hold across rebuilds.
pub type SharedRef = Arc<Shared>;

// The engine keeps one shared state per Output; when the device changes the
// new Output brings its own. This mutex guards the rebuild.
pub type RebuildGuard = Mutex<()>;
