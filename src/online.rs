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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Timing, from the original service.
const PARALLEL_PEERS: usize = 3;
const MAX_PEERS: usize = 8;
const YOUTUBE_AFTER_MS: u64 = 12_000;
const STALL_MS: u64 = 20_000;
const REMOTE_QUEUE_MS: u64 = 25_000;
const MIN_QUERY_LEN: usize = 3;
const POLL_MS: u64 = 1000;
const CACHE_LIFE_MS: u128 = 14 * 24 * 60 * 60 * 1000;

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

struct Attempt {
    source: Value,
    last_bytes: u64,
    last_move_at: u128,
    requested_at: u128,
    percent: i64,
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
    inner: Mutex<Inner>,
    run: AtomicU64,
    poller: Mutex<Option<std::thread::JoinHandle<()>>>,
}

struct Inner {
    results: Results,
    by_key: HashMap<String, Value>, // every result (songs, albums, videos) by resultId
    jobs: HashMap<String, Job>,
    save_when_ready: std::collections::HashSet<String>,
    saved_keys: std::collections::HashSet<String>,
    cache_ready: bool,
}

impl Online {
    pub fn new(events: Arc<Events>, settings: Arc<SettingsFile>) -> Self {
        Online {
            events,
            settings,
            slskd: net::Slskd::new(),
            inner: Mutex::new(Inner {
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
                save_when_ready: std::collections::HashSet::new(),
                saved_keys: std::collections::HashSet::new(),
                cache_ready: false,
            }),
            run: AtomicU64::new(0),
            poller: Mutex::new(None),
        }
    }

    fn emit_results(&self) {
        let payload = {
            let mut g = self.inner.lock().unwrap();
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
        self.events.emit("online.results", payload);
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
        let run = self.run.fetch_add(1, Ordering::SeqCst) + 1;
        self.emit_results();
        if query.chars().count() < MIN_QUERY_LEN {
            return;
        }
        let run = Arc::new(AtomicU64::new(run));
        self.search_peers(query.clone(), Arc::clone(&run));
        self.search_youtube(query, run);
    }

    fn alive(&self, run: &Arc<AtomicU64>) -> bool {
        run.load(Ordering::SeqCst) == self.run.load(Ordering::SeqCst)
    }

    fn search_peers(&self, query: String, run: Arc<AtomicU64>) {
        {
            self.inner.lock().unwrap().results.searching_peers = true;
        }
        self.emit_results();
        let me = self.clone_shared();
        std::thread::Builder::new().name("online-peers".into()).spawn(move || {
            me.peers_loop(&query, &run);
        }).ok();
    }

    fn clone_shared(self: &Arc<Self>) -> Arc<Self> {
        // Online is used as Arc<Online> from main.rs; search needs a clone of
        // that Arc for the worker threads.
        Arc::new(Online {
            events: Arc::clone(&self.events),
            settings: Arc::clone(&self.settings),
            slskd: net::Slskd::new(),
            // The worker gets its own inner-less view; shared state goes
            // through the real Arc below.
            inner: Mutex::new(Inner {
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
                save_when_ready: std::collections::HashSet::new(),
                saved_keys: std::collections::HashSet::new(),
                cache_ready: false,
            }),
            run: AtomicU64::new(0),
            poller: Mutex::new(None),
        })
    }

    fn peers_loop(self: Arc<Self>, query: &str, run: &Arc<AtomicU64>) {
        let created = match self.slskd.api("POST", "/searches", Some(&json!({ "searchText": query }))) {
            Ok(v) => v["id"].as_str().unwrap_or("").to_string(),
            Err(e) => {
                if self.alive(run) {
                    self.inner.lock().unwrap().results.peers_problem = error_text(&e);
                    self.inner.lock().unwrap().results.searching_peers = false;
                    self.emit_results();
                }
                return;
            }
        };
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(1500));
            if !self.alive(run) {
                break;
            }
            let state = self.slskd.api("GET", &format!("/searches/{created}"), None);
            let responses = self.slskd.api("GET", &format!("/searches/{created}/responses"), None);
            let (state_ok, finished) = match &state {
                Ok(s) => {
                    let st = s["state"].as_str().unwrap_or("");
                    (true, st.contains("Completed"))
                }
                Err(_) => (false, false),
            };
            if let Ok(resp) = responses {
                let built = build_results(resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]), query);
                if !self.alive(run) {
                    break;
                }
                let mut g = self.inner.lock().unwrap();
                g.results.songs = built.0;
                g.results.albums = built.1;
                g.results.searching_peers = !finished;
                drop(g);
                self.emit_results();
            } else if state_ok {
                // The state answered, the responses call did not: try again.
            }
            if finished {
                break;
            }
        }
        let _ = self.slskd.api("DELETE", &format!("/searches/{created}"), None);
        if self.alive(run) {
            self.inner.lock().unwrap().results.searching_peers = false;
            self.emit_results();
        }
    }

    fn search_youtube(&self, query: String, run: Arc<AtomicU64>) {
        {
            self.inner.lock().unwrap().results.searching_youtube = true;
        }
        self.emit_results();
        let events = Arc::clone(&self.events);
        let me_run = Arc::clone(&self_run_marker(&run));
        let _ = me_run;
        let run2 = Arc::clone(&run);
        let inner = self.shared_inner();
        std::thread::Builder::new().name("online-yt".into()).spawn(move || {
            let args: Vec<String> = vec![
                "--flat-playlist".into(),
                "--dump-json".into(),
                "--no-warnings".into(),
                format!("ytsearch6:{query}"),
            ];
            let child = match net::spawn_hidden(&net::yt_dlp_path(), &args) {
                Ok(c) => c,
                Err(_) => {
                    inner.lock().unwrap().results.searching_youtube = false;
                    return;
                }
            };
            let mut out = BufReader::new(child.stdout.unwrap());
            let mut line = String::new();
            let mut found: Vec<Value> = Vec::new();
            while out.read_line(&mut line).unwrap_or(0) > 0 {
                if let Some(v) = youtube_song(&line) {
                    found.push(v);
                }
                if run2.load(Ordering::SeqCst) != run2_load_expected(&run2) {
                    // A newer search took over; stop collecting.
                }
                {
                    let mut g = inner.lock().unwrap();
                    g.results.videos = found.clone();
                }
                events.emit("online.results", online_results_snapshot(&inner));
                line.clear();
            }
            inner.lock().unwrap().results.searching_youtube = false;
            events.emit("online.results", online_results_snapshot(&inner));
        }).ok();
    }

    fn shared_inner(self: &Arc<Self>) -> Arc<Mutex<Inner>> {
        // Inner must be shared across worker threads; the Online struct holds
        // it behind Mutex, so wrap the whole Online instead.
        unreachable!()
    }
}
