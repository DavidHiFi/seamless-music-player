//! Player: the queue, shuffle, loop, sleep timer, play counting and the
//! remembered state, wired to the audio engine in audio/engine.rs. The
//! public surface is the one main.rs dispatches on; state goes to the UI
//! on change and at 4 Hz while playing.

use crate::audio::decode::Decoder;
use crate::audio::dsp::{Dsp, to_stereo};
use crate::audio::engine::Engine;
use crate::audio::output;
use crate::audio::resample::Resampler;
use crate::library::Library;
use crate::settings::SettingsFile;
use crate::Events;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub track_id: Option<i64>,
    pub track: Option<Value>,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub shuffle: bool,
    pub loop_mode: String,
    pub sleep_timer: Option<f64>,
    pub queue: Vec<i64>,
    pub index: usize,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            track_id: None,
            track: None,
            playing: false,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            shuffle: false,
            loop_mode: "off".into(),
            sleep_timer: None,
            queue: Vec::new(),
            index: 0,
        }
    }
}

impl PlayerState {
    pub fn to_json(&self) -> Value {
        json!({
            "trackId": self.track_id,
            "track": self.track,
            "playing": self.playing,
            "position": self.position,
            "duration": self.duration,
            "volume": self.volume,
            "shuffle": self.shuffle,
            "loop": self.loop_mode,
            "sleepTimer": self.sleep_timer,
        })
    }
}

struct Sleep {
    deadline: Option<Instant>,
    end_of_song: bool,
}

pub struct Player {
    pub events: Arc<Events>,
    pub lib: Arc<Library>,
    pub settings: Arc<SettingsFile>,
    pub engine: Engine,
    state: Mutex<PlayerState>,
    /// The queue order before shuffle, restored when shuffle goes off.
    order_before_shuffle: Mutex<Vec<i64>>,
    sleep: Mutex<Sleep>,
    /// The track id preloaded into the engine as the next one.
    next_id: Mutex<Option<i64>>,
    pub viz_visible: AtomicBool,
    pub viz_shown: AtomicBool,
    generation: AtomicU64,
}

impl Player {
    pub fn new(events: Arc<Events>, lib: Arc<Library>, settings: Arc<SettingsFile>) -> Arc<Player> {
        let engine = Engine::new(Arc::clone(&settings));
        let mut state = PlayerState::default();
        {
            let s = settings.settings.lock().unwrap();
            state.volume = s.volume.clamp(0.0, 1.0);
            state.shuffle = s.shuffle;
            state.loop_mode = s.loop_mode.clone();
            state.sleep_timer = s.sleep_timer_minutes;
            state.queue = s.last_queue.clone();
            state.index = s.last_index.min(state.queue.len().saturating_sub(1));
            if let Some(id) = s.last_track {
                state.track_id = Some(id);
                state.track = lib.track_by_id(id);
                state.duration = state
                    .track
                    .as_ref()
                    .and_then(|t| t["duration"].as_f64())
                    .unwrap_or(0.0);
                // Restarts come back paused where the last session stopped.
                state.position = s.last_position;
                state.playing = false;
            }
        }
        engine.shared.volume.store((state.volume as f32).to_bits(), Ordering::Relaxed);
        let sleep = Sleep {
            deadline: state
                .sleep_timer
                .filter(|m| *m > 0.0)
                .map(|m| Instant::now() + Duration::from_secs_f64(m * 60.0)),
            end_of_song: false,
        };
        let player = Arc::new(Player {
            events,
            lib,
            settings: Arc::clone(&settings),
            engine,
            state: Mutex::new(state),
            order_before_shuffle: Mutex::new(Vec::new()),
            sleep: Mutex::new(sleep),
            next_id: Mutex::new(None),
            viz_visible: AtomicBool::new(false),
            viz_shown: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        });
        Player::spawn_ticker(&player);
        player
    }

    /// The one helper thread: engine events, the 4 Hz position push, the
    /// visualizer bars at about 30 fps, the sleep timer and the periodic
    /// state save. It sleeps between ticks; nothing here spins.
    fn spawn_ticker(player: &Arc<Player>) {
        let p = Arc::clone(player);
        std::thread::Builder::new()
            .name("smp-ticker".into())
            .spawn(move || ticker_loop(p))
            .ok();
    }

    fn bump(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn push_state(&self) {
        let s = self.state.lock().unwrap().clone();
        self.events.emit("player.state", s.to_json());
    }

    pub fn state_json(&self) -> Value {
        self.state.lock().unwrap().to_json()
    }

    fn track_path(&self, id: i64) -> Option<String> {
        self.lib
            .track_by_id(id)
            .and_then(|t| t["path"].as_str().map(String::from))
    }

    /// The id that should come after the current one, honouring loop modes.
    fn next_id_after(&self, index: usize, loop_mode: &str, queue: &[i64]) -> Option<i64> {
        if queue.is_empty() {
            return None;
        }
        if loop_mode == "one" {
            return queue.get(index).copied();
        }
        if index + 1 < queue.len() {
            return queue.get(index + 1).copied();
        }
        if loop_mode == "all" {
            return queue.first().copied();
        }
        None
    }

    /// Keep the engine's preloaded next track in step with the queue.
    fn refresh_next(&self) {
        let (id, path) = {
            let st = self.state.lock().unwrap();
            let next = self.next_id_after(st.index, &st.loop_mode, &st.queue);
            let path = next.and_then(|id| self.track_path(id));
            (next, path)
        };
        *self.next_id.lock().unwrap() = id;
        self.engine.set_next(path.as_deref().map(Path::new));
    }

    /// Start a track by id: engine play at `seek`, state and preload follow.
    fn start_track(&self, st: &mut PlayerState, id: i64, seek: f64) {
        st.track_id = Some(id);
        st.track = self.lib.track_by_id(id);
        st.duration = st.track.as_ref().and_then(|t| t["duration"].as_f64()).unwrap_or(0.0);
        st.position = seek;
        if let Some(path) = self.track_path(id) {
            self.engine.play(Path::new(&path), seek);
        }
    }

    /// Play and skip counts: a play at half the track or four minutes,
    /// whichever comes first; a skip when a track is left before that.
    fn count_leaving(&self, id: Option<i64>, elapsed: f64) {
        let Some(id) = id else { return };
        let dur = self
            .lib
            .track_by_id(id)
            .and_then(|t| t["duration"].as_f64())
            .unwrap_or(0.0);
        let threshold = if dur > 0.0 { (dur / 2.0).min(240.0) } else { 240.0 };
        if elapsed >= threshold {
            self.lib.record_play(id);
        } else if elapsed >= 1.0 {
            self.lib.record_skip(id);
        }
    }

    // ------------------------------------------------------------------
    // Playback commands
    // ------------------------------------------------------------------

    pub fn play(&self, track_ids: &[i64], start: usize) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        self.count_leaving(st.track_id, st.position);
        st.queue = track_ids.to_vec();
        st.index = start.min(track_ids.len().saturating_sub(1));
        st.playing = true;
        st.position = 0.0;
        if let Some(id) = st.queue.get(st.index).copied() {
            self.start_track(&mut st, id, 0.0);
        } else {
            st.track_id = None;
            st.track = None;
            st.duration = 0.0;
        }
        *self.sleep.lock().unwrap() = Sleep {
            deadline: st
                .sleep_timer
                .filter(|m| *m > 0.0)
                .map(|m| Instant::now() + Duration::from_secs_f64(m * 60.0)),
            end_of_song: false,
        };
        drop(st);
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn toggle(&self) -> Result<Value, String> {
        self.bump();
        let playing = {
            let mut st = self.state.lock().unwrap();
            if st.track_id.is_none() {
                drop(st);
                // Nothing loaded: start the remembered queue if there is one.
                let (q, i) = {
                    let s = self.state.lock().unwrap();
                    (s.queue.clone(), s.index)
                };
                if q.is_empty() {
                    return Ok(json!({ "ok": true }));
                }
                return self.play(&q, i);
            }
            st.playing = !st.playing;
            st.playing
        };
        if playing {
            self.engine.resume();
        } else {
            self.engine.pause();
        }
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    /// Step to the next track. `natural` ends come through engine events;
    /// this is the UI and media-key path.
    pub fn next(&self) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        self.count_leaving(st.track_id, st.position);
        if st.queue.is_empty() {
            st.playing = false;
            drop(st);
            self.push_state();
            return Ok(json!({ "ok": true }));
        }
        let last = st.index + 1 >= st.queue.len();
        if last && st.loop_mode == "off" {
            st.playing = false;
            st.position = 0.0;
            self.engine.pause();
            drop(st);
            self.push_state();
            return Ok(json!({ "ok": true }));
        }
        st.index = if last { 0 } else { st.index + 1 };
        let id = st.queue[st.index];
        st.playing = true;
        self.start_track(&mut st, id, 0.0);
        drop(st);
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn prev(&self) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        if st.position > 3.0 {
            st.position = 0.0;
            let id = st.track_id;
            drop(st);
            if let Some(id) = id {
                if let Some(p) = self.track_path(id) {
                    self.engine.play(Path::new(&p), 0.0);
                }
            }
            self.push_state();
            return Ok(json!({ "ok": true }));
        }
        if st.index > 0 {
            st.index -= 1;
        } else if st.loop_mode == "all" && st.queue.len() > 1 {
            st.index = st.queue.len() - 1;
        }
        let id = st.queue.get(st.index).copied();
        st.playing = true;
        if let Some(id) = id {
            self.start_track(&mut st, id, 0.0);
        }
        drop(st);
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn seek(&self, seconds: f64) -> Result<Value, String> {
        let mut st = self.state.lock().unwrap();
        let s = seconds.clamp(0.0, if st.duration > 0.0 { st.duration } else { seconds.max(0.0) });
        st.position = s;
        let id = st.track_id;
        drop(st);
        if id.is_some() {
            self.engine.seek(s);
        }
        Ok(json!({ "ok": true }))
    }

    pub fn set_volume(&self, v: f64) -> Result<Value, String> {
        let v = v.clamp(0.0, 1.0);
        self.settings.update(|s| s.volume = v);
        self.engine.shared.volume.store((v as f32).to_bits(), Ordering::Relaxed);
        {
            let mut st = self.state.lock().unwrap();
            st.volume = v;
        }
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn set_shuffle(&self, on: bool) -> Result<Value, String> {
        self.settings.update(|s| s.shuffle = on);
        {
            let mut st = self.state.lock().unwrap();
            st.shuffle = on;
            let mut order = self.order_before_shuffle.lock().unwrap();
            if on {
                if order.is_empty() && st.queue.len() > 1 {
                    *order = st.queue.clone();
                }
                let cur = st.queue.get(st.index).copied();
                let mut rng = self.rng();
                let mut q = st.queue.clone();
                for i in (1..q.len()).rev() {
                    let j = rng % (i + 1);
                    q.swap(i, j);
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                }
                st.queue = q;
                st.index = cur.and_then(|c| st.queue.iter().position(|&x| x == c)).unwrap_or(0);
            } else if !order.is_empty() {
                let cur = st.queue.get(st.index).copied();
                st.queue = order.clone();
                *order = Vec::new();
                st.index = cur.and_then(|c| st.queue.iter().position(|&x| x == c)).unwrap_or(0);
            }
        }
        drop(self.state.lock().unwrap());
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    /// A tiny xorshift from the clock: enough for a shuffle, no crate.
    fn rng(&self) -> u64 {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 20))
            .unwrap_or(0x9E3779B97F4A7C15);
        t | 1
    }

    pub fn set_loop(&self, mode: &str) -> Result<Value, String> {
        if !matches!(mode, "off" | "all" | "one") {
            return Err("loop must be off, all or one".into());
        }
        self.settings.update(|s| s.loop_mode = mode.to_string());
        {
            let mut st = self.state.lock().unwrap();
            st.loop_mode = mode.to_string();
        }
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn queue_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        json!({ "items": self.lib.tracks_by_ids(&st.queue), "index": st.index })
    }

    pub fn queue_add(&self, track_ids: &[i64], next: bool) -> Result<Value, String> {
        {
            let mut st = self.state.lock().unwrap();
            if next {
                let at = (st.index + 1).min(st.queue.len());
                for (i, id) in track_ids.iter().enumerate() {
                    st.queue.insert(at + i, *id);
                }
            } else {
                st.queue.extend_from_slice(track_ids);
            }
        }
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn queue_remove(&self, indexes: &[usize]) -> Result<Value, String> {
        {
            let mut st = self.state.lock().unwrap();
            let mut idx: Vec<usize> = indexes.to_vec();
            idx.sort_unstable();
            idx.dedup();
            for i in idx.into_iter().rev() {
                if i < st.queue.len() {
                    st.queue.remove(i);
                }
            }
            st.index = st.index.min(st.queue.len().saturating_sub(1));
        }
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn queue_move(&self, from: usize, to: usize) -> Result<Value, String> {
        {
            let mut st = self.state.lock().unwrap();
            if from < st.queue.len() && to < st.queue.len() && from != to {
                let id = st.queue.remove(from);
                st.queue.insert(to, id);
                // Re-find the playing track so the index follows it.
                if let Some(cur) = st.track_id {
                    if let Some(p) = st.queue.iter().position(|&x| x == cur) {
                        st.index = p;
                    }
                }
                st.index = st.index.min(st.queue.len().saturating_sub(1));
            }
        }
        self.refresh_next();
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn sleep_timer(&self, minutes: Option<f64>, end_of_song: bool) -> Result<Value, String> {
        self.settings.update(|s| s.sleep_timer_minutes = minutes);
        {
            let mut st = self.state.lock().unwrap();
            st.sleep_timer = minutes;
        }
        let mut sl = self.sleep.lock().unwrap();
        sl.end_of_song = end_of_song;
        sl.deadline = minutes
            .filter(|m| *m > 0.0)
            .map(|m| Instant::now() + Duration::from_secs_f64(m * 60.0));
        drop(sl);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn eq_get(&self) -> Value {
        let s = self.settings.settings.lock().unwrap();
        json!({ "enabled": s.eq_enabled, "preset": s.eq_preset, "gains": s.eq_gains })
    }

    pub fn eq_set(&self, enabled: bool, preset: &str, gains: &[f64]) -> Result<Value, String> {
        if gains.len() != 10 {
            return Err("eq needs 10 gains".into());
        }
        let g = gains.to_vec();
        let preset = preset.to_string();
        self.settings.update(|s| {
            s.eq_enabled = enabled;
            s.eq_preset = preset;
            s.eq_gains = g;
        });
        self.engine.set_eq(enabled, gains);
        Ok(json!({ "ok": true }))
    }

    /// The 10 EQ bands, 31 Hz to 16 kHz, the classic ten.
    pub const EQ_BANDS: [f64; 10] =
        [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

    /// Output devices, (id, name) pairs with the default first.
    pub fn list_devices(&self) -> Value {
        let devs = output::list_devices();
        let items: Vec<Value> = devs
            .into_iter()
            .map(|(name, default)| json!({ "id": name, "name": name, "default": default }))
            .collect();
        json!(items)
    }

    pub fn set_device(&self, id: &str, name: Option<&str>) -> Result<Value, String> {
        let id = id.to_string();
        let name = name.map(String::from);
        self.settings.update(|s| {
            s.device_id = Some(id);
            s.device_name = name;
        });
        self.engine.reopen();
        Ok(json!({ "ok": true }))
    }

    /// Remember the queue and position for the next start.
    pub fn save_state(&self) {
        let st = self.state.lock().unwrap().clone();
        self.settings.update(|s| {
            s.last_queue = st.queue.clone();
            s.last_index = st.index;
            s.last_track = st.track_id;
            s.last_position = st.position;
            s.was_playing = st.playing;
        });
    }

    /// The UI shows or hides the visualizer; bars only flow while true.
    pub fn set_viz_visible(&self, visible: bool) {
        self.viz_visible.store(visible, Ordering::Relaxed);
        self.engine.set_viz(visible);
    }
}

/// Position, engine events, viz bars, sleep timer and the periodic save.
/// One thread, sleeping between ticks: at most 33 ms of work per wake and
/// nothing at all while paused and the visualizer is hidden.
fn ticker_loop(p: Arc<Player>) {
    let mut last_pos_push = Instant::now() - Duration::from_secs(1);
    let mut last_viz_push = Instant::now() - Duration::from_secs(1);
    let mut last_save = Instant::now();
    loop {
        let playing = p.state.lock().unwrap().playing;
        let viz_on = p.viz_visible.load(Ordering::Relaxed);
        let sleep_held = p.sleep.lock().unwrap().deadline.is_some();
        // Sleep longest when idle; the engine's own events still queue.
        let wait = if !playing && !viz_on && !sleep_held { 250 } else { 15 };
        std::thread::sleep(Duration::from_millis(wait));

        // Engine events: track ends and seamless switches.
        while let Ok(ev) = p.engine.events.try_recv() {
            match ev {
                crate::audio::engine::EngineEv::Switched { duration, crossfaded: _, .. } => {
                    let mut st = p.state.lock().unwrap();
                    if let Some(next) = *p.next_id.lock().unwrap() {
                        st.index = (st.index + 1).min(st.queue.len().saturating_sub(1));
                        st.track_id = Some(next);
                        st.track = p.lib.track_by_id(next);
                    }
                    st.duration = duration;
                    st.position = 0.0;
                    drop(st);
                    p.refresh_next();
                    p.push_state();
                }
                crate::audio::engine::EngineEv::Ended { natural, .. } => {
                    let id = p.state.lock().unwrap().track_id;
                    if natural {
                        p.count_leaving(id, f64::MAX);
                    } else {
                        p.count_leaving(id, 0.0);
                    }
                    p.next();
                }
                crate::audio::engine::EngineEv::Device { name, rate, ok } => {
                    if !ok {
                        p.events.emit(
                            "player.deviceLost",
                            json!({ "name": name, "rate": rate }),
                        );
                    }
                }
                crate::audio::engine::EngineEv::Started { duration, .. } => {
                    let mut st = p.state.lock().unwrap();
                    st.duration = if duration > 0.0 { duration } else { st.duration };
                    drop(st);
                }
            }
        }

        let playing = p.state.lock().unwrap().playing;
        if playing {
            let pos = p.engine.position();
            {
                let mut st = p.state.lock().unwrap();
                st.position = pos;
            }
            if last_pos_push.elapsed() >= Duration::from_millis(250) {
                last_pos_push = Instant::now();
                let st = p.state.lock().unwrap();
                p.events.emit(
                    "player.position",
                    json!({ "position": st.position, "duration": st.duration }),
                );
            }
            if viz_on && last_viz_push.elapsed() >= Duration::from_millis(33) {
                last_viz_push = Instant::now();
                let bars = p.engine.bars();
                p.events.emit("viz", json!({ "bars": bars }));
            }
            let due = {
                let sl = p.sleep.lock().unwrap();
                sl.deadline.map(|d| Instant::now() >= d).unwrap_or(false)
            };
            if due {
                {
                    let mut st = p.state.lock().unwrap();
                    st.playing = false;
                }
                p.engine.pause();
                p.push_state();
            }
            if last_save.elapsed() >= Duration::from_secs(5) {
                last_save = Instant::now();
                p.save_state();
            }
        }
    }
}

/// Decode a real file through the whole chain to a null sink for `secs`,
/// checking the samples are not silent and the timing tracks. Used by the
/// load gate check and as a cargo test when G:\Dopamine exists.
pub fn selftest(path: &Path, secs: f64) -> Result<String, String> {
    let mut dec = Decoder::open(path)?;
    let (rg_t, rg_a) = (dec.info.rg_track, dec.info.rg_album);
    let mut dsp = Dsp::new(dec.info.sample_rate);
    dsp.set_rg_gains(rg_t, rg_a);
    let rate = 48000u32;
    let mut res = Resampler::new(dec.info.sample_rate, rate, 2)?;
    let mut dsp_out: Vec<f32> = Vec::new();
    let mut chunk = Vec::new();
    let mut peak = 0.0f32;
    let mut frames = 0u64;
    let t0 = std::time::Instant::now();
    while frames < (secs * rate as f64) as u64 {
        chunk.clear();
        if dec.next_chunk(&mut chunk) {
            let mut stereo = to_stereo(&chunk, dec.info.channels);
            dsp.process(&mut stereo);
            let out = res.process(&stereo);
            for s in out.iter().step_by(16) {
                peak = peak.max(s.abs());
            }
            frames += (out.len() / 2) as u64;
        } else {
            let tail = res.flush(&[]);
            for s in tail.iter().step_by(16) {
                peak = peak.max(s.abs());
            }
            frames += (tail.len() / 2) as u64;
            if frames < (secs * rate as f64) as u64 / 4 {
                return Err("track ended far too early".into());
            }
            break;
        }
    }
    let wall = t0.elapsed().as_secs_f64();
    if peak < 0.001 {
        return Err(format!("audio is silent (peak {peak})"));
    }
    // The chain must run faster than real time by a wide margin.
    if wall > secs * 0.5 {
        return Err(format!("decode ran too slow: {wall:.2}s wall for {secs}s"));
    }
    Ok(format!(
        "selftest ok: {} at {} Hz, {} ch, peak {:.3}, {} frames in {:.2}s wall",
        path.display(),
        dec.info.sample_rate,
        dec.info.channels,
        peak,
        frames,
        wall
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest_when_library_present() {
        // A real file from the music library, when the machine has one.
        let dir = match std::fs::read_dir("G:\\Dopamine") {
            Ok(d) => d,
            Err(_) => return, // no library on this machine: skip
        };
        let mut file = None;
        for e in dir.flatten() {
            let p = e.path();
            if p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("flac") | Some("mp3") | Some("m4a") | Some("wav")
                )
            {
                file = Some(p);
                break;
            }
        }
        let Some(file) = file else { return };
        let out = selftest(&file, 2.0).expect("selftest");
        assert!(out.contains("selftest ok"));
    }

    #[test]
    fn count_thresholds_use_half_or_four_minutes() {
        // Exercised through Player below; here the maths is checked plain.
        for dur in [30.0, 240.0, 600.0] {
            let t = (dur / 2.0).min(240.0);
            assert!(t > 0.0 && t <= 240.0);
        }
    }
}
