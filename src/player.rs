//! Player: symphonia decode, rubato resample, cpal WASAPI shared output,
//! gapless, crossfade, ReplayGain, the 10-band EQ, volume, queue, shuffle,
//! loop, sleep timer, remembered state, position events, and the visualizer
//! FFT. Step 4 fills the audio pipeline; the state and command surface is
//! complete here so the shell and the UI can be built against it.

use crate::library::Library;
use crate::settings::SettingsFile;
use crate::Events;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

pub struct Player {
    pub events: Arc<Events>,
    pub lib: Arc<Library>,
    pub settings: Arc<SettingsFile>,
    state: Mutex<PlayerState>,
    /// Set while the audio thread should produce visualizer bars.
    pub viz_visible: AtomicBool,
    pub viz_shown: AtomicBool,
    generation: AtomicU64,
}

impl Player {
    pub fn new(events: Arc<Events>, lib: Arc<Library>, settings: Arc<SettingsFile>) -> Arc<Player> {
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
            }
        }
        Arc::new(Player {
            events,
            lib,
            settings,
            state: Mutex::new(state),
            viz_visible: AtomicBool::new(false),
            viz_shown: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        })
    }

    /// Push the full state to the UI. Position-only updates go through
    /// position_tick (4 Hz) in step 4.
    pub fn push_state(&self) {
        let s = self.state.lock().unwrap().clone();
        self.events.emit("player.state", s.to_json());
    }

    pub fn state_json(&self) -> Value {
        self.state.lock().unwrap().to_json()
    }

    fn bump(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn play(&self, track_ids: &[i64], start: usize) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        st.queue = track_ids.to_vec();
        st.index = start.min(track_ids.len().saturating_sub(1));
        st.track_id = track_ids.get(st.index).copied();
        st.track = st.track_id.and_then(|id| self.lib.track_by_id(id));
        st.position = 0.0;
        st.playing = true;
        // The audio pipeline (decode, output device, gapless chain) starts in
        // step 4; until then the state reflects what would play.
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn toggle(&self) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        st.playing = !st.playing;
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn next(&self) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        if st.index + 1 < st.queue.len() {
            st.index += 1;
            st.track_id = st.queue.get(st.index).copied();
            st.track = st.track_id.and_then(|id| self.lib.track_by_id(id));
            st.position = 0.0;
        } else if st.loop_mode == "all" && !st.queue.is_empty() {
            st.index = 0;
            st.track_id = st.queue.first().copied();
            st.track = st.track_id.and_then(|id| self.lib.track_by_id(id));
            st.position = 0.0;
        } else {
            st.playing = false;
            st.position = 0.0;
        }
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn prev(&self) -> Result<Value, String> {
        self.bump();
        let mut st = self.state.lock().unwrap();
        if st.position > 3.0 {
            st.position = 0.0;
        } else if st.index > 0 {
            st.index -= 1;
            st.track_id = st.queue.get(st.index).copied();
            st.track = st.track_id.and_then(|id| self.lib.track_by_id(id));
            st.position = 0.0;
        }
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn seek(&self, seconds: f64) -> Result<Value, String> {
        let mut st = self.state.lock().unwrap();
        st.position = seconds.clamp(0.0, st.duration);
        Ok(json!({ "ok": true }))
    }

    pub fn set_volume(&self, v: f64) -> Result<Value, String> {
        let v = v.clamp(0.0, 1.0);
        self.settings.update(|s| s.volume = v);
        let mut st = self.state.lock().unwrap();
        st.volume = v;
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn set_shuffle(&self, on: bool) -> Result<Value, String> {
        self.settings.update(|s| s.shuffle = on);
        let mut st = self.state.lock().unwrap();
        st.shuffle = on;
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn set_loop(&self, mode: &str) -> Result<Value, String> {
        if !matches!(mode, "off" | "all" | "one") {
            return Err("loop must be off, all or one".into());
        }
        self.settings.update(|s| s.loop_mode = mode.to_string());
        let mut st = self.state.lock().unwrap();
        st.loop_mode = mode.to_string();
        drop(st);
        self.push_state();
        Ok(json!({ "ok": true }))
    }

    pub fn queue_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        json!({ "items": self.lib.tracks_by_ids(&st.queue), "index": st.index })
    }

    pub fn queue_add(&self, track_ids: &[i64], next: bool) -> Result<Value, String> {
        let mut st = self.state.lock().unwrap();
        if next {
            let at = (st.index + 1).min(st.queue.len());
            for (i, id) in track_ids.iter().enumerate() {
                st.queue.insert(at + i, *id);
            }
        } else {
            st.queue.extend_from_slice(track_ids);
        }
        Ok(json!({ "ok": true }))
    }

    pub fn queue_remove(&self, indexes: &[usize]) -> Result<Value, String> {
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
        Ok(json!({ "ok": true }))
    }

    pub fn queue_move(&self, from: usize, to: usize) -> Result<Value, String> {
        let mut st = self.state.lock().unwrap();
        if from < st.queue.len() && to < st.queue.len() && from != to {
            let id = st.queue.remove(from);
            st.queue.insert(to, id);
            st.index = st.index.min(st.queue.len().saturating_sub(1));
        }
        Ok(json!({ "ok": true }))
    }

    pub fn sleep_timer(&self, minutes: Option<f64>, _end_of_song: bool) -> Result<Value, String> {
        self.settings.update(|s| s.sleep_timer_minutes = minutes);
        let mut st = self.state.lock().unwrap();
        st.sleep_timer = minutes;
        drop(st);
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
        Ok(json!({ "ok": true }))
    }

    /// The 10 EQ bands, 31 Hz to 16 kHz, the classic ten.
    pub const EQ_BANDS: [f64; 10] =
        [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

    /// Output devices, from cpal in step 4.
    pub fn list_devices(&self) -> Value {
        json!([])
    }

    pub fn set_device(&self, id: &str, name: Option<&str>) -> Result<Value, String> {
        let id = id.to_string();
        let name = name.map(String::from);
        self.settings.update(|s| {
            s.device_id = Some(id);
            s.device_name = name;
        });
        Ok(json!({ "ok": true }))
    }

    /// Remember the queue and position for the next start. Called on quit and
    /// every few seconds while playing (step 4 wires the tick).
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
}
