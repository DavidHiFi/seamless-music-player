//! Pure logic ported from Dopamine, algorithm for algorithm. Every function
//! here mirrors the original file named in its comment; the .spec.ts cases
//! run as Rust tests in selftest.rs and cargo test.
//!
//! Ported from:
//! - src\app\data\data-delimiter.ts (delimited strings)
//! - src\app\data\album-key-generator.ts (album keys)
//! - src\app\services\artist\artist-splitter.ts (artist splitting)
//! - src\app\common\metadata\metadata-patcher.ts (unsplittable names)
//! - src\app\common\utils\string-utils.ts (sortable strings)
//! - src\app\ui\pipes\format-playback-time.ts (durations)
//! - src\app\services\playlist\playlist-decoder.ts (m3u)
//! - main\common\settings\default-settings.ts (defaults)

pub const DELIMITER: char = ';';

/// DataDelimiter.toDelimitedString: split every item by ';', trim, drop
/// empties, wrap each part in ';' on both sides, join.
pub fn to_delimited_string(items: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        for part in item.split(DELIMITER) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push(DELIMITER);
            out.push_str(part);
            out.push(DELIMITER);
        }
    }
    out
}

/// DataDelimiter.fromDelimitedString: drop empty parts, unwrap the ';'.
pub fn from_delimited_string(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split(DELIMITER)
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

/// AlbumKeyGenerator.generateAlbumKey: empty when the album title is blank,
/// otherwise the delimited album title plus every lowercased album artist.
pub fn album_key(album_title: &str, album_artists: &[String]) -> String {
    if album_title.trim().is_empty() {
        return String::new();
    }
    let mut items = vec![album_title.to_string()];
    for a in album_artists {
        items.push(a.to_lowercase());
    }
    to_delimited_string(&items)
}

/// CollectionUtils.fromString: the '[ft.][feat.]' bracket list format.
pub fn from_bracket_list(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    if !(s.starts_with('[') && s.ends_with(']')) {
        return vec![s.to_string()];
    }
    s[1..s.len() - 1].split("][").map(|p| p.to_string()).collect()
}

/// The default settings values that drive the logic (default-settings.ts).
pub fn default_artist_split_separators() -> &'static str {
    "[feat.][ft.]"
}

pub fn default_artist_split_exceptions() -> &'static str {
    ""
}

pub fn default_sort_prefixes() -> &'static str {
    "[the][le][les][a][and]"
}

/// Names that must never split across the slash, from metadata-patcher.ts.
pub const UNSPLITTABLE_METADATA: &[&str] = &[
    "AC/DC",
    "De/Vision",
    "Ghost/Light",
    "Axwell /\\ Ingrosso",
    "LOONA 1/3",
    "TR/ST",
    "AU/RA",
    "+/-",
    "A/N【eɪ-ɛn】",
    "Akron/Family",
    "Akron/Family & Angels of Light",
    "AM/FM",
    "Ashes/Dust",
    "B/B/S/",
    "BLCK/MRKT/RGNS",
    "Body/Gate/Head",
    "Body/Head",
    "Born/Dead",
    "Burger/Ink",
    "case/lang/veirs",
    "Chicago / London Underground",
    "Dakota/Dakota",
    "Dark/Light",
    "Decades/Failures",
    "The Denison/Kimball Trio",
    "D-W/L-SS",
    "F/i",
    "Friend / Enemy",
    "GZA/Genius",
    "I/O",
    "I/O3",
    "In/Humanity",
    "Love/Lust",
    "Mirror/Dash",
    "Model/Actress",
    "N/N",
    "Neither/Neither World",
    "P1/E",
    "Sick/Tired",
    "t/e/u/",
    "tide/edit",
    "V/Vm",
    "White/Lichens",
    "White/Light",
    "Yamantaka // Sonic Titan",
    "GG/06",
    "Kiske/Somerville",
    "Black Forest/Black Sea",
    "Internal/External",
    "Pre/verse",
    "Fixmer/McCarthy",
    "Future/Past",
    "Voigt/465",
    "K/DA",
    "m/a/r/r/s",
    "Jizzy Pearl's Love/Hate",
    "Love/Hate",
    "Turilli/Lione Rhapsody",
    "11/5",
    "Kossoff/Kirke/Tetsu/Rabbit",
    "ADD/C",
    "Green/Blue",
    "Cloak/Dagger",
    "Needles//Pins",
    "ИO///sé",
    "20/20",
    "Sko/Torp",
    "Bremer/McCoy",
    "이달의 소녀 1/3",
    "11/29",
    "60/60",
    "Lil 1/2 Dead",
    "Hunter/Game",
    "Frankfurt/Main Underground United",
    ".com/kill",
    "The 2/3rds",
    "A/T/O/S",
    "a / / w a y s",
    "PERMANENT//ZEIMP",
    "A l i c e //",
    "AIBA //",
    "CHAOS//BARISTA",
    "Vantage & The Myracle w/ Hibiya",
    "SOFT://SOUNDS",
    "ANIMAL // MOTHER",
    "Action/Adventure",
    "HUNTR/X",
];

/// MetadataPatcher.joinUnsplittableMetadata: when adjacent parts joined by
/// '/' spell a known unsplittable name, take them out and append the name
/// once, at the end.
pub fn join_unsplittable(parts: &[String]) -> Vec<String> {
    if parts.len() <= 1 {
        return parts.to_vec();
    }
    let lowered: Vec<String> = UNSPLITTABLE_METADATA.iter().map(|s| s.to_lowercase()).collect();
    let mut cumulative: Vec<String> = Vec::new();
    for start in 0..parts.len() {
        for end in start + 1..parts.len() {
            let joined = parts[start..=end].join("/");
            cumulative.push(joined);
        }
    }
    let found: Vec<String> = cumulative
        .into_iter()
        .filter(|j| lowered.iter().any(|u| *u == j.to_lowercase()))
        .collect();
    if found.is_empty() {
        return parts.to_vec();
    }
    let mut to_remove: Vec<String> = Vec::new();
    for item in &found {
        for seg in item.split('/') {
            to_remove.push(seg.to_string());
        }
    }
    let mut out: Vec<String> = parts
        .iter()
        .filter(|p| !to_remove.contains(p))
        .cloned()
        .collect();
    out.extend(found);
    out
}

/// One raw tag value ("A; B" or "A ft. B") turned into the stored list:
/// taglib-style ';' split first, known names rejoined, then separator split.
pub fn split_tag_values(raw: &str, genre: bool) -> Vec<String> {
    let _ = genre;
    let parts: Vec<String> = raw
        .split(DELIMITER)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let rejoined = join_unsplittable(&parts);
    let (separators, exceptions) = if genre {
        (Vec::new(), Vec::new())
    } else {
        (
            from_bracket_list(default_artist_split_separators()),
            from_bracket_list(default_artist_split_exceptions()),
        )
    };
    if separators.is_empty() {
        return rejoined;
    }
    split_artists(&rejoined, &separators, &exceptions)
}

/// ArtistSplitter.splitArtists: exceptions are lifted out first (replaced by
/// a placeholder), then the artist splits on every space-padded separator,
/// case-insensitive. Duplicates drop by lowercase name, order kept.
pub fn split_artists(artists: &[String], separators: &[String], exceptions: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for artist in artists {
        let pieces = split_artist(artist, separators, exceptions);
        for p in pieces {
            let low = p.to_lowercase();
            if !seen.contains(&low) {
                seen.push(low);
                out.push(p);
            }
        }
    }
    out
}

fn split_artist(artist: &str, separators: &[String], exceptions: &[String]) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut work = artist.to_string();
    for exception in exceptions {
        if work.to_lowercase().contains(&exception.to_lowercase()) {
            pieces.push(exception.clone());
            work = replace_ignore_case(&work, exception, "\u{a8}");
        }
    }
    // Separators only count with a space on both sides, like the original.
    let mut current = vec![work];
    for sep in separators {
        let padded = format!(" {} ", sep);
        let mut next = Vec::new();
        for part in current {
            for piece in split_ignore_case(&part, &padded) {
                next.push(piece.trim().to_string());
            }
        }
        current = next;
    }
    pieces.extend(current);
    pieces.retain(|p| p != "\u{a8}");
    pieces
}

fn replace_ignore_case(hay: &str, needle: &str, with: &str) -> String {
    let hay_low = hay.to_lowercase();
    let needle_low = needle.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while i < hay.len() {
        if hay_low[i..].starts_with(&needle_low) {
            out.push_str(with);
            i += needle.len();
        } else {
            let ch_len = hay[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            out.push_str(&hay[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

/// Split on a fixed separator, case-insensitive, ASCII-safe on char bounds.
fn split_ignore_case(hay: &str, sep: &str) -> Vec<String> {
    let hay_low = hay.to_lowercase();
    let sep_low = sep.to_lowercase();
    if sep_low.is_empty() {
        return vec![hay.to_string()];
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let bytes = hay.as_bytes();
    while i < hay.len() {
        if hay_low[i..].starts_with(&sep_low) {
            out.push(hay[start..i].to_string());
            i += sep.len();
            start = i;
        } else {
            i += 1;
            // Stay on a char boundary for the next slice.
            while i < hay.len() && !bytes.get(i).map(|b| (*b & 0xC0) != 0x80).unwrap_or(true) {
                i += 1;
            }
        }
    }
    out.push(hay[start..].to_string());
    out
}

/// StringUtils.getSortableString: lowercase everything, drop the first
/// matched ignore prefix (longest first) when it is followed by a space and
/// something remains. The prefixes come from the sortPrefixes setting.
pub fn sortable_string(s: &str, ignored_prefixes: &[String]) -> String {
    let name = s.trim().to_lowercase();
    if name.is_empty() {
        return String::new();
    }
    let mut prefixes: Vec<&str> = ignored_prefixes.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    prefixes.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for prefix in prefixes {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.starts_with(' ') && !rest.trim().is_empty() {
                return rest.trim_start().to_string();
            }
        }
    }
    name
}

pub fn sortable_string_default(s: &str) -> String {
    sortable_string(s, &from_bracket_list(default_sort_prefixes()))
}

/// FormatPlaybackTimePipe: m:ss under an hour, h:mm:ss from an hour on.
pub fn format_playback_time(seconds: f64) -> String {
    if seconds < 0.0 {
        return "0:00".into();
    }
    let total = seconds.floor() as i64;
    let h = total / 3600;
    let m = (total / 60) % 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One entry from an m3u: the raw line and the full path, relative paths
/// resolved against the playlist folder. Comment lines are skipped; EXTINF
/// metadata is not parsed, exactly like PlaylistDecoder.
#[derive(Debug, Clone, PartialEq)]
pub struct M3uEntry {
    pub raw_path: String,
    pub full_path: String,
}

pub fn read_m3u(body: &str, playlist_dir: &std::path::Path) -> Vec<M3uEntry> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let full = resolve_playlist_path(line, playlist_dir);
        if !full.trim().is_empty() {
            out.push(M3uEntry { raw_path: line.to_string(), full_path: full });
        }
    }
    out
}

/// PlaylistFileManager writes a plain m3u: one absolute path per line.
pub fn write_m3u(paths: &[String]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for p in paths {
        out.push_str(p);
        out.push('\n');
    }
    out
}

fn resolve_playlist_path(entry: &str, playlist_dir: &std::path::Path) -> String {
    let p = std::path::Path::new(entry);
    if p.is_absolute() {
        entry.to_string()
    } else {
        playlist_dir.join(p).to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // AlbumKeyGenerator.spec.ts
    #[test]
    fn album_key_blank_title_is_empty() {
        assert_eq!(album_key("  ", &v(&["Artist 1"])), "");
    }

    #[test]
    fn album_key_title_only() {
        assert_eq!(album_key("The album title", &[]), ";The album title;");
    }

    #[test]
    fn album_key_one_artist() {
        assert_eq!(album_key("The album title", &v(&["Album artist 1"])), ";The album title;;album artist 1;");
    }

    #[test]
    fn album_key_two_artists() {
        assert_eq!(
            album_key("The album title", &v(&["Album artist 1", "Album artist 2"])),
            ";The album title;;album artist 1;;album artist 2;"
        );
    }

    // ArtistSplitter.spec.ts
    #[test]
    fn split_with_pipe_separator_and_exceptions() {
        let artists = v(&[
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
        let out = split_artists(
            &artists,
            &v(&["ft.", "feat.", "&", "|"]),
            &v(&["Artist2 & Artist3", "Artist6 | Artist7"]),
        );
        assert_eq!(out.len(), 12, "{out:?}");
        for expected in ["Artist1", "Artist2 & Artist3", "Artist6 | Artist7", "Artist9", "Artist10"] {
            assert!(out.iter().any(|a| a == expected), "{out:?}");
        }
    }

    #[test]
    fn split_without_pipe_separator() {
        let artists = v(&[
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
        let out = split_artists(&artists, &v(&["ft.", "feat.", "&"]), &v(&["Artist6 | Artist7"]));
        assert_eq!(out.len(), 11, "{out:?}");
        assert!(out.iter().any(|a| a == "Artist5 | Artist6"), "{out:?}");
    }

    #[test]
    fn split_without_separators_keeps_whole_and_dedups() {
        let artists = v(&["Artist1", "artist1", "Artist2", "Artist1 ft. Artist2 feat. Artist3"]);
        let out = split_artists(&artists, &[], &[]);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out.iter().any(|a| a == "Artist1 ft. Artist2 feat. Artist3"));
    }

    // MetadataPatcher.spec.ts behaviour
    #[test]
    fn joins_unsplittable_names() {
        let out = join_unsplittable(&v(&["AC", "DC", "Other"]));
        assert!(out.contains(&"AC/DC".to_string()), "{out:?}");
        assert!(!out.contains(&"AC".to_string()), "{out:?}");
        assert!(out.contains(&"Other".to_string()), "{out:?}");
    }

    #[test]
    fn leaves_normal_names_alone() {
        let out = join_unsplittable(&v(&["Metallica", "Aerosmith"]));
        assert_eq!(out, v(&["Metallica", "Aerosmith"]));
    }

    // StringUtils.getSortableString with sortPrefixes default
    #[test]
    fn sortable_drops_the() {
        assert_eq!(sortable_string_default("The Beatles"), "beatles");
    }

    #[test]
    fn sortable_needs_space_after_prefix() {
        assert_eq!(sortable_string_default("Therion"), "therion");
    }

    #[test]
    fn sortable_drops_a_and_les() {
        assert_eq!(sortable_string_default("A Perfect Circle"), "perfect circle");
        assert_eq!(sortable_string_default("Les Amis"), "amis");
    }

    // FormatPlaybackTimePipe
    #[test]
    fn time_cases() {
        assert_eq!(format_playback_time(0.0), "0:00");
        assert_eq!(format_playback_time(-5.0), "0:00");
        assert_eq!(format_playback_time(65.9), "1:05");
        assert_eq!(format_playback_time(3600.0), "1:00:00");
        assert_eq!(format_playback_time(7325.0), "2:02:05");
    }

    // PlaylistDecoder
    #[test]
    fn m3u_reads_paths_and_resolves_relative() {
        let dir = std::env::temp_dir().join("smp-logic-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entries = read_m3u("#EXTM3U\n#EXTINF:180,x\nC:\\Music\\a.mp3\nb.mp3\n\n", &dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].full_path, "C:\\Music\\a.mp3");
        assert_eq!(entries[1].full_path, dir.join("b.mp3").to_string_lossy());
    }

    #[test]
    fn m3u_roundtrip() {
        let paths = v(&["C:\\Music\\a.mp3", "D:\\x.flac"]);
        let written = write_m3u(&paths);
        assert!(written.starts_with("#EXTM3U\n"));
        assert_eq!(read_m3u(&written, std::path::Path::new("C:\\")).len(), 2);
    }
}
