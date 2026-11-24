//! Online: slskd REST search + peer race + hidden yt-dlp downloads, ported
//! from Dopamine's music-search.service.ts behaviour line for line. Results
//! stream out as online.results events; job progress as download.progress.
//! Played songs live in a cache folder outside the library, so streaming
//! never adds to it; only an explicit download copies into the watched
//! Downloads folder. The slskd login is read from its own config at run
//! time and never copied anywhere.

use crate::net;
use crate::settings::SettingsFile;
use crate::Events;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Timing, from the original service.
const PARALLEL_PEERS: usize = 3;
const MAX_PEERS: usize = 8;
const YOUTUBE_AFTER_MS: u128 = 12_000;
const STALL_MS: u128 = 20_000;
const REMOTE_QUEUE_MS: u128 = 25_000;
const MIN_QUERY_LEN: usize = 3;
const POLL_MS: u64 = 1000;
const CACHE_LIFE_MS: u128 = 14 * 24 * 60 * 60 * 1000;

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

struct Attempt {
    source: Value,
    id: Option<i64>,
    last_bytes: u64,
    last_move_at: u128,
    requested_at: u128,
    percent: i64,
}

impl Attempt {
    /// True while the source still gets a fair chance to move bytes.
    fn active_attempt(&self, now: u128) -> bool {
        now.saturating_sub(self.last_move_at) <= STALL_MS && self.percent < 100
    }
}

struct Job {
    key: String,
    title: String,
    label: String,
    percent: i64,
    active: bool,
    failed: bool,
    paths: Vec<String>,
    mode: String, // play | queue | next | keep
    query: String,
    sources: Vec<Value>,
    next_source: usize,
    tries: Vec<Attempt>,
    youtube_url: Option<String>,
    youtube_started: bool,
    youtube_failed: bool,
    youtube_percent: i64,
    started_at: u128,
    tags: Option<Value>,
    files: Vec<Value>,
    file_done: Vec<bool>,
    next_to_hand: usize,
    requested_at: u128,
    last_move_at: u128,
    child: Option<Arc<Mutex<std::process::Child>>>,
    lines: Arc<Mutex<Vec<String>>>,
    youtube_path: Option<String>,
    target: Option<PathBuf>,
}

impl Job {
    fn to_json(&self) -> Value {
        json!({
            "jobId": self.key,
            "key": self.key,
            "title": self.title,
            "label": self.label,
            "pct": if self.active { self.percent } else { 100 },
            "percent": self.percent,
            "state": if self.failed { "error" } else if self.active { "running" } else { "done" },
            "active": self.active,
            "failed": self.failed,
            "paths": self.paths,
        })
    }
}

struct Results {
    query: String,
    songs: Vec<Value>,
    albums: Vec<Value>,
    videos: Vec<Value>,
    searching_peers: bool,
    searching_youtube: bool,
    peers_problem: String,
}

pub struct Online {
    events: Arc<Events>,
    settings: Arc<SettingsFile>,
    slskd: net::Slskd,
    inner: Arc<Mutex<Inner>>,
    run: Arc<AtomicU64>,
    poller: Arc<AtomicBool>,
}

struct Inner {
    results: Results,
    by_key: HashMap<String, Value>, // every result (songs, albums, videos) by resultId
    jobs: HashMap<String, Job>,
    cached: HashMap<String, String>, // resultId -> path of a finished play job
}

impl Online {
    pub fn new(events: Arc<Events>, settings: Arc<SettingsFile>) -> Self {
        Online {
            events,
            settings,
            slskd: net::Slskd::new(),
            inner: Arc::new(Mutex::new(Inner {
                results: Results {
                    query: String::new(),
                    songs: Vec::new(),
                    albums: Vec::new(),
                    videos: Vec::new(),
                    searching_peers: false,
                    searching_youtube: false,
                    peers_problem: String::new(),
                },
                by_key: HashMap::new(),
                jobs: HashMap::new(),
                cached: HashMap::new(),
            })),
            run: Arc::new(AtomicU64::new(0)),
            poller: Arc::new(AtomicBool::new(false)),
        }
    }

    fn emit_results(&self) {
        emit(&self.inner, &self.events);
    }

    /// The search box. Runs the slskd search and the YouTube search the same
    /// way the original does, results pushed as they arrive.
    pub fn search(&self, q: &str) {
        let query = q.trim().to_string();
        {
            let mut g = self.inner.lock().unwrap();
            if query == g.results.query && !query.is_empty() {
                return;
            }
            g.results = Results {
                query: query.clone(),
                songs: Vec::new(),
                albums: Vec::new(),
                videos: Vec::new(),
                searching_peers: false,
                searching_youtube: false,
                peers_problem: String::new(),
            };
        }
        let epoch = self.run.fetch_add(1, Ordering::SeqCst) + 1;
        self.emit_results();
        if query.chars().count() < MIN_QUERY_LEN {
            return;
        }
        {
            let inner = Arc::clone(&self.inner);
            let events = Arc::clone(&self.events);
            let slskd = net::Slskd::new();
            let run = Arc::clone(&self.run);
            let q2 = query.clone();
            std::thread::Builder::new().name("online-peers".into()).spawn(move || {
                peers_loop(inner, events, slskd, &q2, run, epoch);
            }).ok();
        }
        {
            let inner = Arc::clone(&self.inner);
            let events = Arc::clone(&self.events);
            let run = Arc::clone(&self.run);
            std::thread::Builder::new().name("online-yt".into()).spawn(move || {
                youtube_loop(inner, events, &query, run, epoch);
            }).ok();
        }
    }
}

fn emit(inner: &Mutex<Inner>, events: &Events) {
    let payload = {
        let g = inner.lock().unwrap();
        let r = &g.results;
        json!({
            "query": r.query,
            "soulseek": r.songs,
            "youtube": r.videos,
            "albums": r.albums,
            "searchingPeers": r.searching_peers,
            "searchingYouTube": r.searching_youtube,
            "peersProblem": r.peers_problem,
        })
    };
    events.emit("online.results", payload);
}

fn snapshot(inner: &Mutex<Inner>) -> Value {
    let g = inner.lock().unwrap();
    let r = &g.results;
    json!({
        "query": r.query,
        "soulseek": r.songs,
        "youtube": r.videos,
        "albums": r.albums,
        "searchingPeers": r.searching_peers,
        "searchingYouTube": r.searching_youtube,
        "peersProblem": r.peers_problem,
    })
}

fn error_text(e: &str) -> String {
    if e == "401" || e == "signin" {
        "Soulseek would not sign in. Check its login in the slskd config.".into()
    } else if e.contains("refused") || e.contains("connect") {
        "Soulseek search is off. Start slskd or turn it off in settings.".into()
    } else {
        "Soulseek did not answer.".into()
    }
}

/// One yt-dlp --dump-json line into a result the UI can show.
fn youtube_song(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let id = v["id"].as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    let url = match v["url"].as_str() {
        Some(u) if u.starts_with("http") => u.to_string(),
        _ => format!("https://www.youtube.com/watch?v={id}"),
    };
    let channel = v["channel"]
        .as_str()
        .or_else(|| v["uploader"].as_str())
        .unwrap_or("")
        .trim_end_matches("- Topic")
        .trim()
        .to_string();
    let title = v["title"].as_str().unwrap_or(&id).to_string();
    let dur = v["duration"].as_f64();
    Some(json!({
        "resultId": format!("youtube|{id}"),
        "title": title,
        "artist": channel,
        "album": "",
        "user": "",
        "duration": net::duration_text(dur),
        "bitrate": Value::Null,
        "lossless": false,
        "quality": "YouTube",
        "youtubeUrl": url,
    }))
}

/// Map slskd responses into songs and albums, ranked like the original.
fn build_results(responses: &[Value], query: &str) -> (Vec<Value>, Vec<Value>) {
    let mut songs: HashMap<String, Value> = HashMap::new();
    let mut albums: HashMap<String, Value> = HashMap::new();
    let words: Vec<String> = net::normalize(query, false)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();

    for response in responses {
        let username = response["username"].as_str().unwrap_or("");
        let free_slot = response["hasFreeUploadSlot"].as_bool().unwrap_or(false);
        let queue_len = response["queueLength"].as_i64().unwrap_or(0);
        let speed = response["uploadSpeed"].as_f64().unwrap_or(0.0);
        // One peer's files grouped by their folder, for album candidates.
        let mut folders: HashMap<String, Vec<Value>> = HashMap::new();

        for raw in response["files"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let filename = raw["filename"].as_str().unwrap_or("").to_string();
            let ext = match raw["extension"].as_str() {
                Some(e) => e.trim_start_matches('.').to_ascii_lowercase(),
                None => filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase(),
            };
            if !net::is_audio_file(&filename) {
                continue;
            }
            let folder = filename.rsplit_once('\\').map(|(f, _)| f.to_string()).unwrap_or_default();
            let size = raw["size"].as_f64().unwrap_or(0.0);
            let lossless = net::is_lossless(&filename);
            let quality = net::quality_text(&ext, raw);
            let source = json!({
                "username": username,
                "filename": filename,
                "size": size,
                "folder": folder,
                "freeSlot": free_slot,
                "queueLength": queue_len,
                "speed": speed,
                "lossless": lossless,
                "quality": quality,
            });
            folders.entry(folder.clone()).or_default().push(source.clone());

            let (f_artist, f_title, f_album) = net::parse_name(&filename);
            let key = format!("{}|{}", net::normalize(&f_artist, true), net::normalize(&f_title, true));
            let dur = raw["length"].as_f64();
            let song = songs.entry(key.clone()).or_insert_with(|| json!({
                "resultId": format!("song|{key}"),
                "title": f_title,
                "artist": f_artist,
                "album": f_album,
                "user": username,
                "duration": net::duration_text(dur),
                "bitrate": raw["bitrate"].as_i64(),
                "lossless": lossless,
                "quality": quality,
                "sources": [],
            }));
            if song["duration"].as_str().unwrap_or("").is_empty() {
                song["duration"] = json!(net::duration_text(dur));
            }
            let sources = song["sources"].as_array_mut().unwrap();
            if !sources.iter().any(|s| s["username"].as_str() == Some(username)) {
                sources.push(source);
            }
        }

        // An album candidate from every folder with two or more songs.
        for (folder, mut files) in folders {
            if files.len() < 2 {
                continue;
            }
            files.sort_by(|a, b| a["filename"].as_str().unwrap_or("").cmp(b["filename"].as_str().unwrap_or("")));
            let parts: Vec<&str> = folder.split('\\').filter(|p| !p.is_empty()).collect();
            let (artist, album) = net::parse_folder(&parts);
            let key = format!("{}|{}", net::normalize(&artist, true), net::normalize(&album, true));
            let lossless = files.iter().all(|f| f["lossless"].as_bool().unwrap_or(false));
            let quality = net::most_common(
                &files.iter().filter_map(|f| f["quality"].as_str().map(|s| s.to_string())).collect::<Vec<_>>(),
            );
            let rank = album_rank(free_slot, lossless, files.len());
            let candidate = json!({
                "resultId": format!("album|{username}|{folder}"),
                "title": album,
                "artist": artist,
                "user": username,
                "duration": "",
                "bitrate": Value::Null,
                "lossless": lossless,
                "quality": quality,
                "folder": folder,
                "files": files,
            });
            match albums.get_mut(&key) {
                None => {
                    albums.insert(key, candidate);
                }
                Some(existing) => {
                    let copies = existing["copies"].as_i64().unwrap_or(1) + 1;
                    let old_rank = album_rank(
                        existing["user"]["freeSlot"].as_bool().unwrap_or(false),
                        existing["lossless"].as_bool().unwrap_or(false),
                        existing["files"].as_array().map(|f| f.len()).unwrap_or(0),
                    );
                    if rank > old_rank {
                        let mut candidate = candidate;
                        candidate["copies"] = json!(copies);
                        albums.insert(key, candidate);
                    } else {
                        existing["copies"] = json!(copies);
                    }
                }
            }
        }
    }

    let mut song_list: Vec<Value> = songs
        .into_values()
        .filter(|s| !s["title"].as_str().unwrap_or("").is_empty())
        .collect();
    for song in song_list.iter_mut() {
        let (best_quality, best_lossless, best_user, best_speed, sources_len, any_free) = {
            let sources = song["sources"].as_array_mut().unwrap();
            sources.sort_by(|a, b| source_cost(a).partial_cmp(&source_cost(b)).unwrap_or(std::cmp::Ordering::Equal));
            let first = sources.first();
            (
                first.map(|f| f["quality"].clone()).unwrap_or(Value::Null),
                first.map(|f| f["lossless"].clone()).unwrap_or(Value::Null),
                first.map(|f| f["username"].clone()).unwrap_or(Value::Null),
                first.and_then(|f| f["speed"].as_f64()),
                sources.len(),
                sources.iter().any(|s| s["freeSlot"].as_bool().unwrap_or(false)),
            )
        };
        song["quality"] = best_quality;
        song["lossless"] = best_lossless;
        song["user"] = best_user;
        song["bitrate"] = json!(best_speed.map(|s| (s / 1000.0) as i64));
        let own = net::normalize(&format!("{} {}", song["artist"].as_str().unwrap_or(""), song["title"].as_str().unwrap_or("")), false);
        let matched = words.iter().filter(|w| own.contains(w.as_str())).count();
        let score = if words.is_empty() { 0.0 } else { matched as f64 / words.len() as f64 } * 10.0
            + (1.0 + sources_len as f64).log2() * 3.0
            + if any_free { 2.0 } else { 0.0 };
        song["score"] = json!(score);
    }
    song_list.sort_by(|a, b| b["score"].as_f64().unwrap_or(0.0).partial_cmp(&a["score"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
    song_list.truncate(10);

    let mut album_list: Vec<Value> = albums.into_values().collect();
    album_list.sort_by(|a, b| {
        let ca = a["copies"].as_i64().unwrap_or(1);
        let cb = b["copies"].as_i64().unwrap_or(1);
        cb.cmp(&ca).then_with(|| {
            album_rank(b["lossless"].as_bool().unwrap_or(false), b["lossless"].as_bool().unwrap_or(false), b["files"].as_array().map(|f| f.len()).unwrap_or(0))
                .partial_cmp(&album_rank(a["lossless"].as_bool().unwrap_or(false), a["lossless"].as_bool().unwrap_or(false), a["files"].as_array().map(|f| f.len()).unwrap_or(0)))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    album_list.truncate(6);

    (song_list, album_list)
}

/// Rough seconds until the source is playable; a busy peer costs a long wait.
fn source_cost(s: &Value) -> f64 {
    let size = s["size"].as_f64().unwrap_or(0.0);
    let speed = s["speed"].as_f64().unwrap_or(0.0).max(50_000.0);
    let seconds = size / speed;
    let waiting = if s["freeSlot"].as_bool().unwrap_or(false) { 0.0 } else { 60.0 + s["queueLength"].as_i64().unwrap_or(0) as f64 * 20.0 };
    let bonus = if s["lossless"].as_bool().unwrap_or(false) && seconds < 15.0 { -3.0 } else { 0.0 };
    seconds + waiting + bonus
}

fn album_rank(free_slot: bool, lossless: bool, files: usize) -> f64 {
    free_slot as i32 as f64 * 100.0 + lossless as i32 as f64 * 10.0 + (files.min(30) as f64) / 10.0
}

/// The slskd peer side of a search: post it, collect responses while they
/// arrive, rank them, push them out.
fn peers_loop(
    inner: Arc<Mutex<Inner>>,
    events: Arc<Events>,
    slskd: net::Slskd,
    query: &str,
    run: Arc<AtomicU64>,
    epoch: u64,
) {
    let alive = || run.load(Ordering::SeqCst) == epoch;
    {
        let mut g = inner.lock().unwrap();
        g.results.searching_peers = true;
    }
    emit(&inner, &events);
    let created = match slskd.api("POST", "/searches", Some(&json!({ "searchText": query }))) {
        Ok(v) => v["id"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            if alive() {
                let mut g = inner.lock().unwrap();
                g.results.peers_problem = error_text(&e);
                g.results.searching_peers = false;
                drop(g);
                emit(&inner, &events);
            }
            return;
        }
    };
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(1500));
        if !alive() {
            break;
        }
        let responses = slskd.api("GET", &format!("/searches/{created}/responses"), None);
        let finished = match slskd.api("GET", &format!("/searches/{created}"), None) {
            Ok(s) => s["state"].as_str().unwrap_or("").contains("Completed"),
            Err(_) => false,
        };
        if let Ok(resp) = responses {
            let (mut songs, mut albums) =
                build_results(resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]), query);
            if !alive() {
                break;
            }
            let mut g = inner.lock().unwrap();
            for s in songs.iter_mut() {
                let rid = s["resultId"].as_str().unwrap_or("").to_string();
                g.by_key.insert(rid, s.clone());
            }
            for a in albums.iter_mut() {
                let rid = a["resultId"].as_str().unwrap_or("").to_string();
                g.by_key.insert(rid, a.clone());
            }
            g.results.songs = songs;
            g.results.albums = albums;
            g.results.searching_peers = !finished;
            drop(g);
            emit(&inner, &events);
        }
        if finished {
            break;
        }
    }
    let _ = slskd.api("DELETE", &format!("/searches/{created}"), None);
    if alive() {
        let mut g = inner.lock().unwrap();
        g.results.searching_peers = false;
        drop(g);
        emit(&inner, &events);
    }
}

/// The YouTube side: hidden yt-dlp lists the top videos while typing.
fn youtube_loop(inner: Arc<Mutex<Inner>>, events: Arc<Events>, query: &str, run: Arc<AtomicU64>, epoch: u64) {
    let alive = || run.load(Ordering::SeqCst) == epoch;
    {
        let mut g = inner.lock().unwrap();
        g.results.searching_youtube = true;
    }
    emit(&inner, &events);
    let args: Vec<String> = vec![
        "--flat-playlist".into(),
        "--dump-json".into(),
        "--no-warnings".into(),
        format!("ytsearch6:{query}"),
    ];
    let Ok(child) = net::spawn_hidden(&net::yt_dlp_path(), &args) else {
        let mut g = inner.lock().unwrap();
        g.results.searching_youtube = false;
        drop(g);
        emit(&inner, &events);
        return;
    };
    let mut out = BufReader::new(child.stdout.unwrap());
    let mut line = String::new();
    let mut found: Vec<Value> = Vec::new();
    while out.read_line(&mut line).unwrap_or(0) > 0 {
        if !alive() {
            break;
        }
        if let Some(mut v) = youtube_song(&line) {
            let rid = v["resultId"].as_str().unwrap_or("").to_string();
            {
                let mut g = inner.lock().unwrap();
                g.by_key.insert(rid, v.clone());
            }
            found.push(std::mem::take(&mut v));
            let mut g = inner.lock().unwrap();
            g.results.videos = found.clone();
            drop(g);
            emit(&inner, &events);
        }
        line.clear();
    }
    if alive() {
        let mut g = inner.lock().unwrap();
        g.results.searching_youtube = false;
        drop(g);
        emit(&inner, &events);
    }
}

// ---------------------------------------------------------------------
// Streaming and downloads
// ---------------------------------------------------------------------

static PLAYER: OnceLock<std::sync::Weak<crate::player::Player>> = OnceLock::new();

/// The player registers itself, so a finished play job can start at once.
pub fn register_player(p: &Arc<crate::player::Player>) {
    let _ = PLAYER.set(Arc::downgrade(p));
}

fn play_now(paths: &[PathBuf]) {
    if let Some(p) = PLAYER.get().and_then(|w| w.upgrade()) {
        let _ = p.play_paths(paths);
    }
}

impl Online {
    /// Stream now: play the result as soon as a copy is in the cache folder.
    pub fn stream(&self, rid: &str) -> Value {
        let cached = self.inner.lock().unwrap().cached.get(rid).cloned();
        if let Some(path) = cached {
            let path = PathBuf::from(path);
            if path.exists() {
                play_now(&[path]);
                return json!({ "ok": true, "started": true });
            }
        }
        let res = match self.inner.lock().unwrap().by_key.get(rid).cloned() {
            Some(r) => r,
            None => return json!({ "ok": false, "error": "The result is gone. Search again." }),
        };
        match self.start_job(rid, &res, "play") {
            Ok(job) => json!({ "ok": true, "jobId": job }),
            Err(e) => json!({ "ok": false, "error": e }),
        }
    }

    /// Download into the watched Downloads folder, like a normal download.
    pub fn download(&self, rid: &str) -> Value {
        let res = match self.inner.lock().unwrap().by_key.get(rid).cloned() {
            Some(r) => r,
            None => return json!({ "ok": false, "error": "The result is gone. Search again." }),
        };
        match self.start_job(rid, &res, "keep") {
            Ok(job) => json!({ "ok": true, "jobId": job }),
            Err(e) => json!({ "ok": false, "error": e }),
        }
    }

    pub fn cancel(&self, job_id: &str) {
        let mut g = self.inner.lock().unwrap();
        g.jobs.remove(job_id);
    }

    fn start_job(&self, rid: &str, res: &Value, mode: &str) -> Result<String, String> {
        let title = res["title"].as_str().unwrap_or("Online song").to_string();
        let query = self.inner.lock().unwrap().results.query.clone();
        let (sources, youtube_url): (Vec<Value>, Option<String>) = if let Some(u) = res["youtubeUrl"].as_str() {
            (Vec::new(), Some(u.to_string()))
        } else if let Some(files) = res["files"].as_array() {
            (files.clone(), None)
        } else {
            let mut s = res["sources"].as_array().cloned().unwrap_or_default();
            s.sort_by(|a, b| source_cost(a).partial_cmp(&source_cost(b)).unwrap_or(std::cmp::Ordering::Equal));
            (s, None)
        };
        let target = if mode == "play" {
            Some(crate::settings::app_dir().join("cache"))
        } else {
            Some(self.downloads_target())
        };
        if let Some(t) = &target {
            let _ = std::fs::create_dir_all(t);
        }
        let job = Job {
            key: rid.to_string(),
            title,
            label: String::new(),
            percent: 0,
            active: true,
            failed: false,
            paths: Vec::new(),
            mode: mode.to_string(),
            query,
            sources,
            next_source: 0,
            tries: Vec::new(),
            youtube_url,
            youtube_started: false,
            youtube_failed: false,
            youtube_percent: 0,
            started_at: now_ms(),
            tags: None,
            files: Vec::new(),
            file_done: Vec::new(),
            next_to_hand: 0,
            requested_at: 0,
            last_move_at: 0,
            child: None,
            lines: Arc::new(Mutex::new(Vec::new())),
            youtube_path: None,
            target,
        };
        self.ensure_poller();
        self.inner.lock().unwrap().jobs.insert(rid.to_string(), job);
        Ok(rid.to_string())
    }

    /// Where finished downloads go: the setting, else next to the library.
    fn downloads_target(&self) -> PathBuf {
        if let Some(f) = self.settings.settings.lock().unwrap().downloads_folder.clone() {
            return PathBuf::from(f);
        }
        let folders = self.settings.settings.lock().unwrap().folders.clone();
        if let Some(first) = folders.first() {
            return Path::new(first).join("Downloads");
        }
        crate::settings::app_dir().join("Downloads")
    }

    fn ensure_poller(&self) {
        if !self.poller.swap(true, Ordering::SeqCst) {
            let inner = Arc::clone(&self.inner);
            let events = Arc::clone(&self.events);
            let settings = Arc::clone(&self.settings);
            let slskd = net::Slskd::new();
            let poller = Arc::clone(&self.poller);
            std::thread::Builder::new().name("online-jobs".into()).spawn(move || {
                job_loop(inner, events, settings, slskd, poller);
            }).ok();
        }
    }
}

/// One tick of the job engine every POLL_MS. Everything here is non-blocking
/// API calls and file moves; the thread exits when nothing is left.
fn job_loop(
    inner: Arc<Mutex<Inner>>,
    events: Arc<Events>,
    settings: Arc<SettingsFile>,
    slskd: net::Slskd,
    poller: Arc<AtomicBool>,
) {
    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));
        let mut progress: Vec<Value> = Vec::new();
        {
            let mut g = inner.lock().unwrap();
            let now = now_ms();
            let query = g.results.query.clone();
            let keys: Vec<String> = g.jobs.keys().cloned().collect();
            let mut done_keys: Vec<String> = Vec::new();
            for key in &keys {
                let Some(j) = g.jobs.get_mut(key) else { continue };
                if !j.active {
                    continue;
                }
                if j.youtube_started {
                    tick_youtube(j, now);
                } else if j.sources.is_empty() && j.files.is_empty() {
                    // No peer sources at all: YouTube it is.
                    start_youtube(j, &query);
                } else {
                    tick_slskd(j, &slskd, now, &query);
                }
                if !j.active {
                    done_keys.push(key.clone());
                }
                progress.push(j.to_json());
            }
            for key in &done_keys {
                let job = g.jobs.remove(key);
                if let Some(j) = job {
                    if j.failed {
                        // Nothing more to do; the UI shows the failure.
                    } else if let Some(t) = &j.target {
                        let _ = t;
                    }
                }
            }
            drop(g);
        }
        for p in progress {
            events.emit("download.progress", p);
        }
        let idle = {
            let g = inner.lock().unwrap();
            g.jobs.values().all(|j| !j.active) && g.jobs.values().all(|j| j.child.is_none())
        };
        if idle {
            poller.store(false, Ordering::SeqCst);
            return;
        }
        let _ = &settings;
    }
}

/// Start the hidden yt-dlp child for a job's YouTube fallback.
fn start_youtube(j: &mut Job, query: &str) {
    j.youtube_started = true;
    j.label = if j.youtube_url.is_some() || j.sources.is_empty() {
        "Getting it from YouTube".to_string()
    } else {
        "Also trying YouTube".to_string()
    };
    let cache = crate::settings::app_dir().join("cache").join("YouTube");
    let _ = std::fs::create_dir_all(&cache);
    let folders = net::Slskd::read_folders();
    let incomplete = folders.1;
    let target = j.youtube_url.clone().unwrap_or_else(|| format!("ytsearch1:{query}"));
    let mut args: Vec<String> = vec![
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--force-overwrites".into(),
        "--no-mtime".into(),
        "--encoding".into(),
        "utf-8".into(),
        "--newline".into(),
        "--progress".into(),
        "--progress-template".into(),
        "download:enh %(progress._percent_str)s".into(),
        "--print".into(),
        "after_move:enhpath %(filepath)s".into(),
        // m4a, because phones cannot play opus from the cache.
        "-f".into(),
        "bestaudio[ext=m4a]/bestaudio".into(),
        "-x".into(),
        "--audio-format".into(),
        "m4a".into(),
        "--embed-metadata".into(),
        "-P".into(),
        cache.to_string_lossy().to_string(),
        "-P".into(),
        format!("temp:{incomplete}"),
        "-o".into(),
        "%(title)s.%(ext)s".into(),
        target,
    ];
    if let Some(tags) = &j.tags {
        args.splice(20..20, net::tag_args(tags));
    }
    match net::spawn_hidden(&net::yt_dlp_path(), &args) {
        Ok(mut child) => {
            let lines = Arc::clone(&j.lines);
            if let Some(out) = child.stdout.take() {
                std::thread::Builder::new().name("online-yt-read".into()).spawn(move || {
                    let mut r = BufReader::new(out);
                    let mut l = String::new();
                    while r.read_line(&mut l).unwrap_or(0) > 0 {
                        lines.lock().unwrap().push(l.trim().to_string());
                        l.clear();
                    }
                }).ok();
            }
            if let Some(err) = child.stderr.take() {
                std::thread::Builder::new().name("online-yt-err".into()).spawn(move || {
                    let mut r = BufReader::new(err);
                    let mut l = String::new();
                    while r.read_line(&mut l).unwrap_or(0) > 0 {
                        l.clear();
                    }
                }).ok();
            }
            j.child = Some(Arc::new(Mutex::new(child)));
            j.requested_at = now_ms();
        }
        Err(_) => {
            j.active = false;
            j.failed = true;
            j.label = "yt-dlp is missing, so nothing could be fetched".into();
        }
    }
}

/// Read the YouTube child's progress lines; finish the job when it lands.
fn tick_youtube(j: &mut Job, now: u128) {
    let lines: Vec<String> = std::mem::take(&mut *j.lines.lock().unwrap());
    for l in lines {
        if let Some(pct) = l.strip_prefix("download:enh ") {
            let n: String = pct.trim().trim_end_matches('%').trim().to_string();
            if let Ok(p) = n.parse::<i64>() {
                j.youtube_percent = p.clamp(0, 99);
                j.percent = j.youtube_percent;
            }
        } else if l.starts_with("after_move:enhpath ") {
            j.youtube_path = Some(l.trim_start_matches("after_move:enhpath ").trim().to_string());
        }
    }
    let Some(child) = &j.child else { return };
    let done = child.lock().unwrap().try_wait().map(|s| s.is_some()).unwrap_or(false);
    let stalled = now.saturating_sub(j.requested_at) > STALL_MS * 3 && j.percent == 0;
    if done || stalled {
        j.active = false;
        if let Some(c) = &j.child {
            let _ = c.lock().unwrap().kill();
        }
        if stalled {
            j.failed = true;
            j.label = "YouTube did not answer".into();
            return;
        }
        let Some(path) = j.youtube_path.clone().filter(|p| Path::new(p).exists()) else {
            j.failed = true;
            j.label = "Nothing came back from YouTube".into();
            return;
        };
        finish(j, &[PathBuf::from(path)]);
    }
}

/// The slskd side: enqueue the next source, watch the transfer, finish or
/// move on when it stalls.
fn tick_slskd(j: &mut Job, slskd: &net::Slskd, now: u128, query: &str) {
    // Nothing enqueued yet: queue the next best source.
    let pending = j.tries.len() < j.next_source && j.tries.last().map(|t| t.percent < 100).unwrap_or(false);
    let _ = pending;
    if j.tries.is_empty() || j.tries.last().map(|t| !t.active_attempt(now)).unwrap_or(false) {
        if j.next_source >= j.sources.len() {
            if j.youtube_started || j.youtube_failed {
                if !j.active {
                    return;
                }
            }
            start_youtube(j, query);
            return;
        }
        let source = j.sources[j.next_source].clone();
        j.next_source += 1;
        let username = source["username"].as_str().unwrap_or("").to_string();
        let filename = source["filename"].as_str().unwrap_or("").to_string();
        let size = source["size"].as_f64().unwrap_or(0.0);
        let body = json!([{ "username": username, "files": [{ "filename": filename, "size": size }] }]);
        match slskd.api("POST", "/transfers/downloads/batches", Some(&body)) {
            Ok(_) => {
                j.tries.push(Attempt {
                    source,
                    id: None,
                    last_bytes: 0,
                    last_move_at: now,
                    requested_at: now,
                    percent: 0,
                });
                j.requested_at = now;
                j.last_move_at = now;
                j.label = format!("Asking {username}");
            }
            Err(e) => {
                // Peer refused straight away: next source on the next tick.
                j.label = error_text(&e);
            }
        }
        return;
    }
    // Watch the running transfer.
    let transfers = match slskd.api("GET", "/transfers/downloads", None) {
        Ok(t) => t,
        Err(_) => return,
    };
    let Some(t) = j.tries.last_mut() else { return };
    let want_user = t.source["username"].as_str().unwrap_or("").to_string();
    let want_file = t.source["filename"].as_str().unwrap_or("").to_string();
    let mut found: Option<(i64, i64, String)> = None; // (id, percent, status)
    for user in transfers.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if user["username"].as_str() != Some(want_user.as_str()) {
            continue;
        }
        for dir in user["directories"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            for f in dir["files"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                if f["filename"].as_str() == Some(want_file.as_str()) {
                    found = Some((
                        f["id"].as_i64().unwrap_or(0),
                        f["percentComplete"].as_i64().unwrap_or(0),
                        f["status"].as_str().unwrap_or("").to_string(),
                    ));
                }
            }
        }
    }
    match found {
        Some((id, pct, status)) => {
            t.id = Some(id);
            t.percent = pct;
            if pct > 0 {
                t.last_move_at = now;
            }
            j.percent = pct;
            j.label = format!("{status} ({pct}%)");
            if status == "Completed" || pct >= 100 {
                if let Some(path) = find_downloaded(slskd, &want_file) {
                    t.percent = 100;
                    j.active = false;
                    finish(j, &[path]);
                } else {
                    // The file is complete but not where we look yet; wait a tick.
                }
            } else if now.saturating_sub(t.last_move_at) > STALL_MS {
                // Stalled: drop the transfer, try the next source.
                if t.id.unwrap_or(0) > 0 {
                    let _ = slskd.api(
                        "DELETE",
                        &format!("/transfers/downloads/{}/{}?remove=true", want_user, t.id.unwrap_or(0)),
                        None,
                    );
                }
                j.tries.pop();
            }
        }
        None => {
            // The transfer vanished from the queue (peer cleared it): next.
            if now.saturating_sub(t.requested_at) > REMOTE_QUEUE_MS {
                j.tries.pop();
            }
        }
    }
}

/// A completed slskd transfer lands in its downloads folder; find it.
fn find_downloaded(slskd: &net::Slskd, want_file: &str) -> Option<PathBuf> {
    let (downloads, _) = net::Slskd::read_folders();
    let base = PathBuf::from(&downloads);
    if !base.exists() {
        return None;
    }
    let name = want_file.rsplit('\\').next().unwrap_or(want_file);
    let mut best: Option<(SystemTime, PathBuf)> = None;
    let mut stack = vec![base];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n.to_string_lossy() == name).unwrap_or(false) {
                if let Ok(meta) = e.metadata() {
                    let m = meta.modified().unwrap_or(UNIX_EPOCH);
                    if best.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                        best = Some((m, p));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// A job finished with files on disk: play them or move them into place.
fn finish(j: &mut Job, paths: &[PathBuf]) {
    j.percent = 100;
    if j.mode == "play" {
        play_now(paths);
        return;
    }
    let Some(target) = j.target.clone() else {
        j.failed = true;
        return;
    };
    let _ = std::fs::create_dir_all(&target);
    let mut moved: Vec<String> = Vec::new();
    for src in paths {
        let Some(name) = src.file_name() else { continue };
        let dest = target.join(name);
        if std::fs::rename(src, &dest).is_err() {
            if std::fs::copy(src, &dest).is_ok() {
                let _ = std::fs::remove_file(src);
            } else {
                j.failed = true;
                j.label = "The file could not be moved into the Downloads folder".into();
                return;
            }
        }
        moved.push(dest.to_string_lossy().to_string());
    }
    j.paths = moved;
}
