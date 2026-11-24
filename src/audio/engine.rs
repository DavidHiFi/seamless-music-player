//! The decode engine: one worker thread that decodes, resamples and feeds
//! the output ring. Gapless by pre-opening the next track, crossfade by
//! mixing the two streams with the equal-power curve from dsp. Sleeps
//! whenever there is nothing to do; no busy loops, no allocation in the
//! audio path.

use crate::audio::decode::Decoder;
use crate::audio::dsp::{crossfade_gains, Dsp, to_stereo};
use crate::audio::output::{self, Shared};
use crate::audio::resample::Resampler;
use crate::audio::viz::{Viz, BARS};
use crate::settings::SettingsFile;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Cmd {
    Play { path: PathBuf, seek: f64 },
    SetNext { path: Option<PathBuf> },
    Pause,
    Resume,
    Seek { seconds: f64 },
    Eq { enabled: bool, gains: Vec<f64> },
    ReplayGain { mode: String, preamp: f64 },
    Viz { on: bool },
    Reopen,
    Exit,
}

/// Engine to player: what happened on the audio thread.
pub enum EngineEv {
    Started { duration: f64, rate: u32 },
    /// Seamless or crossfaded step into the preloaded next track.
    Switched { duration: f64, elapsed: f64, crossfaded: bool },
    /// Track over, nothing preloaded. elapsed is the played seconds.
    Ended { elapsed: f64, natural: bool },
    /// The output device appeared, changed or died.
    Device { name: String, rate: u32, ok: bool },
}

/// Position ledger: maps frames the device has consumed to seconds inside
/// the current track, surviving gapless switches and seeks.
struct Ledger {
    marks: VecDeque<(u64, f64)>,
}

impl Ledger {
    fn new() -> Ledger {
        Ledger { marks: VecDeque::new() }
    }
    fn mark(&mut self, written: u64, pos: f64) {
        self.marks.push_back((written, pos));
        while self.marks.len() > 8 {
            self.marks.pop_front();
        }
    }
    fn position(&self, consumed: u64, rate: u32) -> f64 {
        let mut best: Option<(u64, f64)> = None;
        for &(w, p) in &self.marks {
            if w <= consumed {
                best = Some((w, p));
            }
        }
        match best {
            Some((w, p)) => p + (consumed - w) as f64 / rate as f64,
            None => self.marks.front().map(|&(_, p)| p).unwrap_or(0.0),
        }
    }
}

pub struct EngineShared {
    pub ledger: Mutex<Ledger>,
    pub bars: Mutex<[f32; BARS]>,
    pub device: Mutex<(String, u32, bool)>,
}

pub struct Engine {
    tx: Sender<Cmd>,
    pub shared: Arc<Shared>,
    pub state: Arc<EngineShared>,
    pub events: Receiver<EngineEv>,
}

struct Worker {
    rx: Receiver<Cmd>,
    ev: Sender<EngineEv>,
    shared: Arc<Shared>,
    state: Arc<EngineShared>,
    settings: Arc<SettingsFile>,
    dec: Option<Decoder>,
    res: Option<Resampler>,
    dsp: Dsp,
    next: Option<(PathBuf, Decoder, Resampler, Dsp)>,
    out_rate: u32,
    playing: bool,
    viz_on: bool,
    viz: Option<Viz>,
    written: u64,
    xfading: bool,
    /// The preloaded next track that the player expects to hear next.
    next_path: Option<PathBuf>,
    chunk: Vec<f32>,
    mix_buf: Vec<f32>,
    out_buf: Vec<f32>,
}

impl Worker {
    fn open_output(&mut self, preferred: Option<String>) -> bool {
        match output::open(preferred.as_deref()) {
            Ok(o) => {
                let vol = self.shared.volume.load(Ordering::Relaxed);
                o.shared.volume.store(vol, Ordering::Relaxed);
                o.shared.fade_target.store(0.0f32.to_bits(), Ordering::Relaxed);
                self.out_rate = o.rate;
                let ok = self.shared.alive.swap(true, Ordering::Relaxed);
                let _ = ok;
                // New shared state: swap our handle and reset the ledger so
                // position keeps running from where it was.
                let pos = {
                    let mut l = self.state.ledger.lock().unwrap();
                    let p = l.marks.back().map(|&(_, p)| p).unwrap_or(0.0);
                    l.mark(0, p);
                    p
                };
                let _ = pos;
                self.shared = Arc::clone(&o.shared);
                o.shared.swap_ring(Arc::new(crate::audio::ring::Ring::new(
                    o.rate as usize * o.channels as usize / 4,
                    o.channels as usize,
                )));
                self.written = 0;
                self.res = None;
                if self.dec.is_some() {
                    self.res = Some(Resampler::new(self.cur_rate(), o.rate, 2).unwrap_or_else(|_| {
                        Resampler::new(44100, o.rate, 2).unwrap()
                    }));
                }
                *self.state.device.lock().unwrap() =
                    (o.device_name.clone(), o.rate, true);
                let _ = self.ev.send(EngineEv::Device { name: o.device_name.clone(), rate: o.rate, ok: true });
                true
            }
            Err(e) => {
                eprintln!("output: {e}");
                self.shared.alive.store(false, Ordering::Relaxed);
                *self.state.device.lock().unwrap() = (String::new(), 0, false);
                let _ = self.ev.send(EngineEv::Device { name: String::new(), rate: 0, ok: false });
                false
            }
        }
    }

    fn cur_rate(&self) -> u32 {
        self.dec.as_ref().map(|d| d.info.sample_rate).unwrap_or(44100)
    }

    fn load(&mut self, path: &Path, seek: f64) {
        self.xfading = false;
        self.next = None;
        match Decoder::open(path) {
            Ok(mut d) => {
                if seek > 0.0 {
                    let _ = d.seek(seek);
                }
                let (rg_t, rg_a) = (d.info.rg_track, d.info.rg_album);
                self.dsp.set_rg_gains(rg_t, rg_a);
                let dur = d.info.duration;
                self.res = Some(
                    Resampler::new(d.info.sample_rate, self.out_rate, 2)
                        .unwrap_or_else(|_| Resampler::new(44100, self.out_rate, 2).unwrap()),
                );
                self.dec = Some(d);
                // Fresh ring: the callback stops reading the old track at
                // once and the ledger restarts at the seek point.
                let ch = self.shared.channels.load(Ordering::Relaxed).max(1) as usize;
                self.shared.swap_ring(Arc::new(crate::audio::ring::Ring::new(
                    self.out_rate as usize * ch / 4,
                    ch,
                )));
                self.written = 0;
                self.state.ledger.lock().unwrap().mark(0, seek);
                let _ = self.ev.send(EngineEv::Started { duration: dur, rate: self.out_rate });
            }
            Err(e) => {
                eprintln!("decode: {e}");
                self.dec = None;
                let _ = self.ev.send(EngineEv::Ended { elapsed: 0.0, natural: false });
            }
        }
    }

    fn preload(&mut self, path: Option<PathBuf>) {
        self.next_path = path.clone();
        match path {
            Some(p) => match Decoder::open(&p) {
                Ok(d) => {
                    let res = Resampler::new(d.info.sample_rate, self.out_rate, 2)
                        .unwrap_or_else(|_| Resampler::new(44100, self.out_rate, 2).unwrap());
                    let mut nd = Dsp::new(self.out_rate);
                    let s = self.settings.settings.lock().unwrap();
                    nd.set_eq(s.eq_enabled, &s.eq_gains);
                    nd.set_replaygain(&s.replaygain, s.preamp_db);
                    nd.set_rg_gains(d.info.rg_track, d.info.rg_album);
                    drop(s);
                    self.next = Some((p, d, res, nd));
                }
                Err(e) => {
                    eprintln!("preload: {e}");
                    self.next = None;
                }
            },
            None => self.next = None,
        }
    }

    /// One feed step: decode, dsp, resample, ring. Returns true when it fed
    /// anything. Handles the gapless swap and the crossfade mix.
    fn feed(&mut self) -> bool {
        let dec = match self.dec.as_mut() {
            Some(d) => d,
            None => return false,
        };
        let in_chunk = self.res.as_ref().map(|r| r.input_chunk()).unwrap_or(2048);
        self.chunk.clear();
        let mut eof = false;
        if dec.next_chunk(&mut self.chunk) {
            let mut stereo = to_stereo(&self.chunk, dec.info.channels);
            self.dsp.process(&mut stereo);
            stereo
        } else {
            eof = true;
            Vec::new()
        };

        // Crossfade: near the end, mix the next track in over the fade.
        let fade = {
            let s = self.settings.settings.lock().unwrap();
            (s.crossfade_seconds, s.gapless)
        };
        let pos = dec.frames_out as f64 / dec.info.sample_rate as f64;
        let remaining = dec.info.duration - pos;
        if eof && !self.xfading {
            if let Some((_, nd, nres, _)) = self.next.take() {
                // Gapless step: the decoder becomes the next track; the ring
                // keeps flowing so there is no gap.
                let dur = nd.info.duration;
                let (rg_t, rg_a) = (nd.info.rg_track, nd.info.rg_album);
                self.dsp.set_rg_gains(rg_t, rg_a);
                self.res = Some(nres);
                self.dec = Some(nd);
                let elapsed = pos;
                self.state.ledger.lock().unwrap().mark(self.written, 0.0);
                let _ = self
                    .ev
                    .send(EngineEv::Switched { duration: dur, elapsed, crossfaded: false });
                return true;
            }
            // Nothing preloaded: flush the tail and report the end once the
            // device has drained the ring.
            let mut stereo = Vec::new();
            if let Some(r) = self.res.as_mut() {
                stereo = r.flush(&to_stereo(&self.chunk, 2));
            }
            let n = self.write(&stereo);
            self.drain_or_end(pos);
            return n > 0;
        }

        // Start a crossfade when the next track is waiting and it is time.
        if !self.xfading && fade > 0.0 && remaining > 0.0 && remaining <= fade {
            if let Some((_, ref mut nd, _, _)) = self.next {
                let _ = nd;
                self.xfading = true;
            }
        }

        let stereo = if self.xfading {
            self.mix_crossfade(stereo, fade)
        } else {
            stereo
        };

        if !stereo.is_empty() {
            let out = self.res.as_mut().map(|r| r.process(&stereo)).unwrap_or_default();
            self.write(&out);
            // Viz bars from the device-rate stream.
            if self.viz_on {
                if let Some(v) = self.viz.as_mut() {
                    if v.feed(&out) {
                        *self.state.bars.lock().unwrap() = v.bars;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Mix the outgoing and incoming streams over the crossfade window.
    fn mix_crossfade(&mut self, cur: Vec<f32>, fade: f64) -> Vec<f32> {
        let dec = self.dec.as_ref().expect("xfade without a decoder");
        let fade_frames = (fade * dec.info.sample_rate as f64) as usize;
        let pos_frames = dec.frames_out as usize;
        let done = pos_frames.min(fade_frames) as f64 / fade_frames.max(1) as f64;
        let (g_out, _g_in) = crossfade_gains(done);
        let mut next_chunk = Vec::new();
        let mut switched = false;
        if let Some((_, nd, _, _)) = self.next.as_mut() {
            if nd.next_chunk(&mut next_chunk) {
                let mut ns = to_stereo(&next_chunk, nd.info.channels);
                if let Some((_, _, _, n_dsp)) = self.next.as_mut() {
                    n_dsp.process(&mut ns);
                }
                self.mix_buf = ns;
            } else {
                self.mix_buf.clear();
                switched = true;
            }
        } else {
            self.mix_buf.clear();
        }
        // Resample the incoming stream to the device rate with its own
        // resampler, then match frame counts with the outgoing stream.
        let mut out = cur.clone();
        if !self.mix_buf.is_empty() {
            if let Some((_, _, nres, _)) = self.next.as_mut() {
                let n_out = nres.process(&self.mix_buf);
                let frames = (out.len() / 2).min(n_out.len() / 2);
                let (_, g_in) = crossfade_gains(done);
                for f in 0..frames {
                    out[f * 2] = out[f * 2] * g_out as f32 + n_out[f * 2] * g_in as f32;
                    out[f * 2 + 1] =
                        out[f * 2 + 1] * g_out as f32 + n_out[f * 2 + 1] * g_in as f32;
                }
            }
        }
        if switched || done >= 1.0 {
            // The fade is over: the incoming track becomes the current one.
            if let Some((p, nd, nres, ndsp)) = self.next.take() {
                let dur = nd.info.duration;
                let _ = p;
                self.dsp = ndsp;
                self.res = Some(nres);
                self.dec = Some(nd);
                self.xfading = false;
                self.state.ledger.lock().unwrap().mark(self.written, 0.0);
                let _ = self.ev.send(EngineEv::Switched { duration: dur, elapsed: done * fade, crossfaded: true });
                self.next_path = None;
            }
        }
        out
    }

    /// Write interleaved stereo into the ring, dropping frames that do not
    /// fit (the ring is sized to hold hundreds of ms, so this is rare).
    fn write(&mut self, stereo: &[f32]) -> usize {
        let ch = self.shared.channels.load(Ordering::Relaxed).max(1) as usize;
        self.out_buf = crate::audio::dsp::from_stereo(stereo, ch);
        let n = self.shared.ring.read().unwrap().write(&self.out_buf);
        self.written += n as u64;
        n
    }

    fn drain_or_end(&mut self, elapsed: f64) {
        // Let the device play the last buffered frames, then end.
        loop {
            if self.rx.try_recv().is_ok() {
                // A new command arrived mid-drain; leave the end to it.
                let _ = elapsed;
                return;
            }
            let used = self.shared.ring.read().unwrap().used_frames();
            if used == 0 {
                let _ = self.ev.send(EngineEv::Ended { elapsed, natural: true });
                self.dec = None;
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The worker loop: commands first, then feed while playing. Sleeps scale
/// with how much the ring still holds, so the thread idles when paused and
/// wakes a few dozen times a second while playing.
fn run(mut w: Worker) {
    let mut dead_since: Option<Instant> = None;
    loop {
        // Block when there is nothing to play; otherwise poll lightly.
        if !w.playing || w.dec.is_none() {
            match w.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(cmd) => {
                    if !handle(&mut w, cmd) {
                        return;
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        match w.rx.try_recv() {
            Ok(cmd) => {
                if !handle(&mut w, cmd) {
                    return;
                }
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        // Device came back or died: reopen, at most every 2 s.
        if !w.shared.alive.load(Ordering::Relaxed) {
            if dead_since.map(|t| t.elapsed() > Duration::from_secs(2)).unwrap_or(true) {
                dead_since = Some(Instant::now());
                let pref = w.settings.settings.lock().unwrap().device_id.clone();
                w.open_output(pref);
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        dead_since = None;

        let fed = w.feed();
        if !w.playing {
            continue;
        }
        // Sleep enough to keep the ring comfortably full without spinning.
        let ch = w.shared.channels.load(Ordering::Relaxed).max(1) as usize;
        let used = w.shared.ring.read().unwrap().used_frames();
        let cap = w.shared.ring.read().unwrap().capacity_frames();
        let target = cap * 3 / 4;
        let ms = if !fed && used < target {
            4 // starved: poll fast until audio flows again
        } else if used < target {
            8
        } else {
            20
        };
        let _ = ch;
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// Handle one command. Returns false when the worker should exit.
fn handle(w: &mut Worker, cmd: Cmd) -> bool {
    match cmd {
        Cmd::Exit => return false,
        Cmd::Play { path, seek } => {
            w.playing = true;
            w.load(&path, seek);
            // Fade in from silence.
            w.shared.fade_cur.store(0.0f32.to_bits(), Ordering::Relaxed);
            w.shared.fade_target.store(1.0f32.to_bits(), Ordering::Relaxed);
        }
        Cmd::SetNext { path } => w.preload(path),
        Cmd::Pause => {
            w.playing = false;
            // Fade the buffered tail out under the pause.
            w.shared.fade_target.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        Cmd::Resume => {
            w.playing = true;
            w.shared.fade_target.store(1.0f32.to_bits(), Ordering::Relaxed);
        }
        Cmd::Seek { seconds } => {
            if let Some(d) = w.dec.as_mut() {
                let _ = d.seek(seconds);
                let ch = w.shared.channels.load(Ordering::Relaxed).max(1) as usize;
                w.shared.swap_ring(Arc::new(crate::audio::ring::Ring::new(
                    w.out_rate as usize * ch / 4,
                    ch,
                )));
                w.written = 0;
                w.state.ledger.lock().unwrap().mark(0, seconds);
                w.xfading = false;
                if let Some(r) = w.res.as_mut() {
                    let _ = r.flush(&[]);
                }
            }
        }
        Cmd::Eq { enabled, gains } => {
            w.dsp.set_eq(enabled, &gains);
            if let Some((_, _, _, nd)) = w.next.as_mut() {
                nd.set_eq(enabled, &gains);
            }
        }
        Cmd::ReplayGain { mode, preamp } => {
            w.dsp.set_replaygain(&mode, preamp);
            if let Some((_, _, _, nd)) = w.next.as_mut() {
                nd.set_replaygain(&mode, preamp);
            }
        }
        Cmd::Viz { on } => {
            w.viz_on = on;
            if on && w.viz.is_none() {
                w.viz = Some(Viz::new(w.out_rate));
            }
            if !on {
                *w.state.bars.lock().unwrap() = [0.0; BARS];
            }
        }
        Cmd::Reopen => {
            let pref = w.settings.settings.lock().unwrap().device_id.clone();
            w.open_output(pref);
        }
    }
    true
}

impl Engine {
    /// Starts the worker and the first output device. A missing device is
    /// not fatal: the worker keeps retrying and the player still runs.
    pub fn new(settings: Arc<SettingsFile>) -> Engine {
        let (tx, rx) = std::sync::mpsc::channel();
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        let (rate, ch) = {
            let s = settings.settings.lock().unwrap();
            let _ = s;
            (48000u32, 2u32)
        };
        let shared = Shared::new(rate, ch, rate as usize * ch as usize / 4);
        let state = Arc::new(EngineShared {
            ledger: Mutex::new(Ledger::new()),
            bars: Mutex::new([0.0; BARS]),
            device: Mutex::new((String::new(), 0, false)),
        });
        let mut w = Worker {
            rx,
            ev: ev_tx,
            shared: Arc::clone(&shared),
            state: Arc::clone(&state),
            settings: Arc::clone(&settings),
            dec: None,
            res: None,
            dsp: Dsp::new(rate),
            next: None,
            out_rate: rate,
            playing: false,
            viz_on: false,
            viz: None,
            written: 0,
            xfading: false,
            next_path: None,
            chunk: Vec::new(),
            mix_buf: Vec::new(),
            out_buf: Vec::new(),
        };
        {
            let s = settings.settings.lock().unwrap();
            w.dsp.set_eq(s.eq_enabled, &s.eq_gains);
            w.dsp.set_replaygain(&s.replaygain, s.preamp_db);
        }
        {
            let pref = settings.settings.lock().unwrap().device_id.clone();
            w.open_output(pref);
        }
        std::thread::Builder::new()
            .name("smp-audio".into())
            .spawn(move || run(w))
            .ok();
        Engine { tx, shared, state, events: ev_rx }
    }

    pub fn play(&self, path: &Path, seek: f64) {
        let _ = self.tx.send(Cmd::Play { path: path.to_path_buf(), seek });
    }
    pub fn set_next(&self, path: Option<&Path>) {
        let _ = self.tx.send(Cmd::SetNext { path: path.map(|p| p.to_path_buf()) });
    }
    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }
    pub fn seek(&self, seconds: f64) {
        let _ = self.tx.send(Cmd::Seek { seconds });
    }
    pub fn set_eq(&self, enabled: bool, gains: &[f64]) {
        let _ = self.tx.send(Cmd::Eq { enabled, gains: gains.to_vec() });
    }
    pub fn set_replaygain(&self, mode: &str, preamp: f64) {
        let _ = self.tx.send(Cmd::ReplayGain { mode: mode.into(), preamp });
    }
    pub fn set_viz(&self, on: bool) {
        let _ = self.tx.send(Cmd::Viz { on });
    }
    pub fn reopen(&self) {
        let _ = self.tx.send(Cmd::Reopen);
    }
    pub fn exit(&self) {
        let _ = self.tx.send(Cmd::Exit);
    }

    /// Seconds inside the current track, from the device's own frame count.
    pub fn position(&self) -> f64 {
        let rate = self.shared.rate.load(Ordering::Relaxed).max(1);
        let consumed = self.shared.played_frames();
        self.state.ledger.lock().unwrap().position(consumed, rate)
    }

    pub fn device(&self) -> (String, u32, bool) {
        self.state.device.lock().unwrap().clone()
    }

    pub fn bars(&self) -> [f32; BARS] {
        *self.state.bars.lock().unwrap()
    }

    /// True when the output stream has signalled an error (device gone).
    pub fn output_dead(&self) -> bool {
        !self.shared.alive.load(Ordering::Relaxed)
    }
}
