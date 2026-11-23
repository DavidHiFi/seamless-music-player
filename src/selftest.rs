//! --selftest: headless checks with no window. Uses a temp data folder
//! (SMP_DATA_DIR), so the owner's data is never touched. Covers the pure
//! logic units against the original .spec.ts cases, the library database,
//! and the Dopamine import on a temp copy of its database.

use crate::logic;
use serde_json::{json, Value};

struct Checker {
    pass: usize,
    fails: Vec<String>,
}

impl Checker {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        if ok {
            self.pass += 1;
        } else {
            self.fails.push(format!("{name}: {detail}"));
        }
    }
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn run() -> i32 {
    let mut c = Checker { pass: 0, fails: Vec::new() };

    // ---- AlbumKeyGenerator.spec.ts ----
    c.check(
        "albumKey blank title",
        logic::album_key("  ", &v(&["Artist 1"])) == "",
        format!("{:?}", logic::album_key("  ", &v(&["Artist 1"]))),
    );
    c.check(
        "albumKey title only",
        logic::album_key("The album title", &[]) == ";The album title;",
        format!("{:?}", logic::album_key("The album title", &[])),
    );
    c.check(
        "albumKey one artist",
        logic::album_key("The album title", &v(&["Album artist 1"])) == ";The album title;;album artist 1;",
        format!("{:?}", logic::album_key("The album title", &v(&["Album artist 1"]))),
    );
    c.check(
        "albumKey two artists",
        logic::album_key("The album title", &v(&["Album artist 1", "Album artist 2"]))
            == ";The album title;;album artist 1;;album artist 2;",
        format!("{:?}", logic::album_key("The album title", &v(&["Album artist 1", "Album artist 2"]))),
    );

    // ---- DataDelimiter (used everywhere) ----
    c.check(
        "delimited join",
        logic::to_delimited_string(&v(&["A", "B; C"])) == ";A;;B;;C;",
        format!("{:?}", logic::to_delimited_string(&v(&["A", "B; C"]))),
    );
    c.check(
        "delimited split",
        logic::from_delimited_string(";A;;B;") == v(&["A", "B"]),
        format!("{:?}", logic::from_delimited_string(";A;;B;")),
    );

    // ---- ArtistSplitter.spec.ts, case 1 ----
    // separators [ft.][feat.][&][|], exceptions [Artist2 & Artist3][Artist6 | Artist7]
    let artists: Vec<String> = v(&[
        "Artist1",
        "artist1",
        "Artist2",
        "Artist1 ft. Artist2 feat. Artist3",
        "artist1 FT. artist2 & Artist3",
        "Artist2 ft. Artist3 & Artist4",
        "artist4",
        "Artist3 & Artist5",
        "Artist5 | Artist6",
        "Artist6 | Artist7",
        "",
        "Artist2ft.Artist3& Artist4",
        " Artist9   ft.  Artist10   ",
    ]);
    let case1 = logic::split_artists(
        &artists,
        &v(&["ft.", "feat.", "&", "|"]),
        &v(&["Artist2 & Artist3", "Artist6 | Artist7"]),
    );
    c.check("split case1 count", case1.len() == 12, format!("{case1:?}"));
    for expected in ["Artist1", "Artist2 & Artist3", "Artist6 | Artist7", "Artist9", "Artist10", "Artist2ft.Artist3& Artist4"] {
        c.check("split case1 item", case1.iter().any(|a| a == expected), format!("{case1:?}"));
    }
    // ---- ArtistSplitter.spec.ts, case 2: pipe not a separator ----
    let case2 = logic::split_artists(&artists, &v(&["ft.", "feat.", "&"]), &v(&["Artist6 | Artist7"]));
    c.check("split case2 count", case2.len() == 11, format!("{case2:?}"));
    c.check(
        "split case2 keeps pipe",
        case2.iter().any(|a| a == "Artist5 | Artist6"),
        format!("{case2:?}"),
    );
    // ---- ArtistSplitter.spec.ts, case 3: no separators, still deduped ----
    let case3 = logic::split_artists(&artists, &[], &[]);
    c.check("split case3 count", case3.len() == 12, format!("{case3:?}"));
    c.check(
        "split case3 keeps whole",
        case3.iter().any(|a| a == "Artist1 ft. Artist2 feat. Artist3"),
        format!("{case3:?}"),
    );

    // ---- MetadataPatcher: unsplittable names ----
    let joined = logic::join_unsplittable(&v(&["AC", "DC", "Other"]));
    c.check(
        "join unsplittable",
        joined.contains(&"AC/DC".to_string()) && !joined.contains(&"AC".to_string()) && joined.contains(&"Other".to_string()),
        format!("{joined:?}"),
    );

    // ---- StringUtils.getSortableString with the default prefixes ----
    c.check(
        "sortable the",
        logic::sortable_string_default("The Beatles") == "beatles",
        format!("{:?}", logic::sortable_string_default("The Beatles")),
    );
    c.check(
        "sortable prefix must be followed by space",
        logic::sortable_string_default("Therion") == "therion",
        format!("{:?}", logic::sortable_string_default("Therion")),
    );
    c.check(
        "sortable a",
        logic::sortable_string_default("A Perfect Circle") == "perfect circle",
        format!("{:?}", logic::sortable_string_default("A Perfect Circle")),
    );
    c.check(
        "sortable les",
        logic::sortable_string_default("Les Amis") == "amis",
        format!("{:?}", logic::sortable_string_default("Les Amis")),
    );

    // ---- FormatPlaybackTimePipe.spec.ts style cases ----
    c.check("time zero", logic::format_playback_time(0.0) == "0:00", logic::format_playback_time(0.0));
    c.check("time negative", logic::format_playback_time(-5.0) == "0:00", logic::format_playback_time(-5.0));
    c.check("time 65", logic::format_playback_time(65.9) == "1:05", logic::format_playback_time(65.9));
    c.check("time hour", logic::format_playback_time(3600.0) == "1:00:00", logic::format_playback_time(3600.0));
    c.check("time long", logic::format_playback_time(7325.0) == "2:02:05", logic::format_playback_time(7325.0));

    // ---- PlaylistDecoder: m3u with comments and a relative path ----
    let tmp = std::env::temp_dir().join(format!("smp-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("pl");
    std::fs::create_dir_all(&dir).unwrap();
    let body = "#EXTM3U\n#EXTINF:180,Artist - Title\nC:\\Music\\a.mp3\nb.mp3\n\n";
    let entries = logic::read_m3u(body, &dir);
    c.check(
        "m3u read",
        entries.len() == 2
            && entries[0].full_path == "C:\\Music\\a.mp3"
            && entries[1].full_path == dir.join("b.mp3").to_string_lossy(),
        format!("{entries:?}"),
    );
    let written = logic::write_m3u(&["C:\\Music\\a.mp3".into(), "D:\\x.flac".into()]);
    c.check(
        "m3u roundtrip",
        logic::read_m3u(&written, &dir).len() == 2,
        written.clone(),
    );

    // ---- library + import on a temp copy ----
    std::env::set_var("SMP_DATA_DIR", &tmp);
    let events = crate::Events::new();
    let db = tmp.join("library.db");
    match crate::library::Library::open(&db, events) {
        Ok(lib) => {
            let added = lib.add_folder("C:\\Music");
            c.check("library addFolder", added.is_ok(), format!("{added:?}"));
            let folders = lib.folders();
            c.check(
                "library folders count",
                folders[0]["path"] == json!("C:\\Music"),
                format!("{folders}"),
            );
            let ov = lib.overview();
            c.check("library overview zero", ov["tracks"] == json!(0), format!("{ov}"));
            // Import from a copy of the real Dopamine database when one is
            // present; counts must match in vs out.
            let dopamine_db = dirs::data_dir()
                .map(|d| d.join("Dopamine Enhanced").join("Dopamine.db"))
                .unwrap_or_default();
            if dopamine_db.exists() {
                let copy = tmp.join("dopamine-copy.db");
                match std::fs::copy(&dopamine_db, &copy) {
                    Ok(_) => match crate::migrate::run_import(&copy, None, &lib) {
                        Ok(rep) => {
                            let ti = rep["counts"]["tracksIn"].as_i64().unwrap_or(-1);
                            let to = rep["counts"]["tracksOut"].as_i64().unwrap_or(-1);
                            c.check(
                                "import tracks in == out",
                                ti == to && ti > 0,
                                format!("in {ti} out {to}"),
                            );
                        }
                        Err(e) => c.check("import runs", false, e),
                    },
                    Err(e) => c.check("import db copy", false, e.to_string()),
                }
            } else {
                c.check("import skipped (no Dopamine.db)", true, String::new());
            }
        }
        Err(e) => c.check("library opens", false, e),
    }
    let _ = std::fs::remove_dir_all(&tmp);

    if c.fails.is_empty() {
        println!("selftest: {} checks passed", c.pass);
        0
    } else {
        for f in &c.fails {
            println!("FAIL {f}");
        }
        1
    }
}
