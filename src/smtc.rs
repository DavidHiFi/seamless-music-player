//! SMTC: Windows System Media Transport Controls. Media keys and the
//! Windows flyout show title, artist, album and artwork, and their play,
//! pause, next and previous buttons drive the player. The worker thread
//! owns the WinRT objects; Smtc itself only carries the latest state, so
//! it stays safe inside main.rs's Mutex. The player registers itself, so
//! main.rs needs no wiring beyond Smtc::new().

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::core::{factory, interface, GUID, HRESULT, HSTRING, IUnknown, Interface};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// The interop entry: SMTC for our window, so the flyout binds to us.
#[interface("ddb0472d-c911-4a1f-86d9-9c771bb1c8c9")]
unsafe trait ISystemMediaTransportControlsInterop: windows::core::IUnknown {
    fn GetForWindow(
        &self,
        appwindow: HWND,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> HRESULT;
}

/// The player hands itself over here; smtc.rs owns this registry so the
/// button handler can reach the player without touching main.rs.
static PLAYER: OnceLock<Weak<crate::player::Player>> = OnceLock::new();

pub fn register_player(p: &Arc<crate::player::Player>) {
    let _ = PLAYER.set(Arc::downgrade(p));
}

/// Latest state from the player plus the stop flag.
struct Latest {
    state: Mutex<Option<Value>>,
}

pub struct Smtc {
    latest: Arc<Latest>,
    stop: Arc<AtomicBool>,
}

impl Smtc {
    pub fn new() -> Smtc {
        let latest = Arc::new(Latest { state: Mutex::new(None) });
        let stop = Arc::new(AtomicBool::new(false));
        let w = Worker {
            latest: Arc::clone(&latest),
            stop: Arc::clone(&stop),
        };
        std::thread::Builder::new()
            .name("smp-smtc".into())
            .spawn(move || worker(w))
            .ok();
        Smtc { latest, stop }
    }

    /// Push the current player state. Position changes are throttled by
    /// the worker, so this can be called as often as player.state fires.
    pub fn update(&self, state: &Value) {
        *self.latest.state.lock().unwrap() = Some(state.clone());
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Worker {
    latest: Arc<Latest>,
    stop: Arc<AtomicBool>,
}

fn now_100ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64 / 100 + 11_644_473_600_000_000)
        .unwrap_or(0)
}

fn worker(w: Worker) {
    // Everything WinRT happens on this one thread, in the MTA.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    // Find the main window; it may not exist yet at startup.
    let mut hwnd = HWND::default();
    for _ in 0..40 {
        if w.stop.load(Ordering::Relaxed) {
            return;
        }
        unsafe {
            hwnd = FindWindowW(None, windows::core::w!("Seamless Music Player")).unwrap_or_default();
        }
        if !hwnd.is_invalid() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let smtc = attach(hwnd);
    let Some(smtc) = smtc else { return };

    // Media keys and flyout buttons drive the player through the registry.
    let handler = {
        TypedEventHandler::new(move |_: &Option<SystemMediaTransportControls>,
                                    args: &Option<SystemMediaTransportControlsButtonPressedEventArgs>|
              -> windows::core::Result<()> {
            let Some(args) = args else { return Ok(()) };
            let button = match args.Button() {
                Ok(b) => b,
                Err(_) => return Ok(()),
            };
            if let Some(p) = PLAYER.get().and_then(|w| w.upgrade()) {
                match button {
                    windows::Media::SystemMediaTransportControlsButton::Play => {
                        if !p.state_json()["playing"].as_bool().unwrap_or(false) {
                            let _ = p.toggle();
                        }
                    }
                    windows::Media::SystemMediaTransportControlsButton::Pause => {
                        if p.state_json()["playing"].as_bool().unwrap_or(false) {
                            let _ = p.toggle();
                        }
                    }
                    windows::Media::SystemMediaTransportControlsButton::Next => {
                        let _ = p.next();
                    }
                    windows::Media::SystemMediaTransportControlsButton::Previous => {
                        let _ = p.prev();
                    }
                    _ => {}
                }
                p.push_state();
            }
            Ok(())
        })
    };
    let _ = smtc.ButtonPressed(&handler);
    let _ = smtc.SetIsPlayEnabled(true);
    let _ = smtc.SetIsPauseEnabled(true);
    let _ = smtc.SetIsNextEnabled(true);
    let _ = smtc.SetIsPreviousEnabled(true);

    // Push loop: metadata on change, position every second while playing.
    let mut last_key = String::new();
    let mut last_push = std::time::Instant::now() - Duration::from_secs(2);
    loop {
        if w.stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
        let state = w.latest.state.lock().unwrap().clone();
        let Some(state) = state else { continue };
        let playing = state["playing"].as_bool().unwrap_or(false);
        let title = state["track"]["title"].as_str().unwrap_or("");
        let artist = state["track"]["artist"].as_str().unwrap_or("");
        let album = state["track"]["album"].as_str().unwrap_or("");
        let art = state["track"]["artworkId"].as_str().unwrap_or("");
        let pos = state["position"].as_f64().unwrap_or(0.0);
        let dur = state["duration"].as_f64().unwrap_or(0.0);

        let key = format!("{title}|{artist}|{album}|{art}|{playing}");
        if key != last_key {
            last_key = key.clone();
            if push_metadata(&smtc, title, artist, album, art, playing).is_err() {
                break; // the control is gone (window closed); stop pushing
            }
            last_push = std::time::Instant::now() - Duration::from_secs(2);
        } else if playing && last_push.elapsed() >= Duration::from_secs(1) {
            last_push = std::time::Instant::now();
            let _ = push_timeline(&smtc, pos, dur);
        }
    }
    // Leave the flyout in a stopped state on the way out.
    let _ = smtc.SetPlaybackStatus(MediaPlaybackStatus::Stopped);
}

/// The SMTC control for the main window, or from a hidden media player
/// when the window was not found.
fn attach(hwnd: HWND) -> Option<SystemMediaTransportControls> {
    unsafe {
        if !hwnd.is_invalid() {
            if let Ok(interop) =
                factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()
            {
                let mut out: *mut std::ffi::c_void = std::ptr::null_mut();
                let hr = interop.GetForWindow(hwnd, &SystemMediaTransportControls::IID, &mut out);
                if hr.is_ok() && !out.is_null() {
                    return Some(windows_core::Type::from_abi(out as _).ok()?);
                }
            }
        }
        // Hidden player fallback: its own SMTC, its own buttons off so the
        // two do not fight over the flyout.
        if let Ok(player) = windows::Media::Playback::MediaPlayer::new() {
            if let Ok(cm) = player.CommandManager() {
                let _ = cm.SetIsEnabled(false);
            }
            return player.SystemMediaTransportControls().ok();
        }
        None
    }
}

fn push_metadata(
    smtc: &SystemMediaTransportControls,
    title: &str,
    artist: &str,
    album: &str,
    art_id: &str,
    playing: bool,
) -> windows::core::Result<()> {
    smtc.SetPlaybackStatus(if playing {
        MediaPlaybackStatus::Playing
    } else {
        MediaPlaybackStatus::Paused
    })?;
    let updater = smtc.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?;
    let music = updater.MusicProperties()?;
    music.SetTitle(&HSTRING::from(title))?;
    music.SetArtist(&HSTRING::from(artist))?;
    music.SetAlbumArtist(&HSTRING::from(artist))?;
    music.SetAlbumTitle(&HSTRING::from(album))?;
    if !art_id.is_empty() {
        if let Ok(uri) = Uri::CreateUri(&HSTRING::from(format!("smp://art/{art_id}.jpg"))) {
            if let Ok(stream) = windows::Storage::Streams::RandomAccessStreamReference::CreateFromUri(&uri) {
                let _ = updater.SetThumbnail(&stream);
            }
        }
    }
    updater.Update()
}

fn push_timeline(
    smtc: &SystemMediaTransportControls,
    pos: f64,
    dur: f64,
) -> windows::core::Result<()> {
    let tl = SystemMediaTransportControlsTimelineProperties::new()?;
    tl.SetPosition(TimeSpan { Duration: (pos * 10_000_000.0) as i64 })?;
    tl.SetEndTime(TimeSpan { Duration: (dur * 10_000_000.0) as i64 })?;
    tl.SetMinSeekTime(TimeSpan { Duration: 0 })?;
    tl.SetMaxSeekTime(TimeSpan { Duration: (dur * 10_000_000.0) as i64 })?;
    smtc.UpdateTimelineProperties(&tl)
}
