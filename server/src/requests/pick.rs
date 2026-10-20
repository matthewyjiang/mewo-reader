//! Pure release scoring. No HTTP and no clock.

use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;

pub(crate) struct Wanted<'a> {
    pub title: &'a str,
    pub author: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Release {
    pub guid: String,
    #[allow(dead_code)]
    pub indexer_id: i64,
    pub indexer: String,
    pub title: String,
    pub size: u64,
    pub protocol: String,
    pub download_url: Option<String>,
    pub grabs: Option<i64>,
    #[allow(dead_code)]
    pub seeders: Option<i64>,
    #[serde(default)]
    pub categories: Vec<Category>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Category {
    pub id: i64,
}

const MIN_RELEASE: u64 = 50 * 1024;
const MAX_RELEASE: u64 = 200 * 1024 * 1024;
const BIG_RELEASE: u64 = 50 * 1024 * 1024;

const STOP: &[&str] = &[
    "a", "an", "the", "of", "and", "or", "to", "in", "on", "for", "with", "by", "from",
];
const BAD_FORMAT: &[&str] = &[
    "pdf",
    "mobi",
    "azw3",
    "cbz",
    "cbr",
    "mp3",
    "m4b",
    "audiobook",
];
const LANGUAGES: &[&str] = &["german", "french", "spanish", "italian", "dutch"];
const MARKERS: &[&str] = &["summary", "workbook", "sampler"];
/// Extra words that are not a different book. `a`/`an`/`the`/`of` also live in [`STOP`].
const EDITION: &[&str] = &[
    "a",
    "an",
    "the",
    "of",
    "edition",
    "deluxe",
    "anniversary",
    "illustrated",
    "unabridged",
    "novel",
];

/// The search string sent to Prowlarr: `<title without subtitle> <author surname>`.
/// A missing Open Library author is the word `Unknown` and is left off.
pub(crate) fn query(w: &Wanted) -> String {
    let title = w.title.split(':').next().unwrap_or(w.title);
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let surname = if w.author.trim().eq_ignore_ascii_case("unknown") {
        ""
    } else {
        w.author.split_whitespace().last().unwrap_or("")
    };
    match (title.is_empty(), surname.is_empty()) {
        (true, true) => String::new(),
        (true, false) => surname.to_string(),
        (false, true) => title,
        (false, false) => format!("{title} {surname}"),
    }
}

/// Best release not in `tried`, or `None`.
pub(crate) fn pick<'r>(
    w: &Wanted,
    releases: &'r [Release],
    tried: &[String],
    max_bytes: u64,
) -> Option<&'r Release> {
    let want = significant(w.title);
    if want.is_empty() {
        return None;
    }
    let work_tokens = token_set(w.title);
    let mut best: Option<(i64, i64, u64, &str, usize)> = None;
    for (index, release) in releases.iter().enumerate() {
        if tried.iter().any(|guid| guid == &release.guid) {
            continue;
        }
        let tokens = tokens(&release.title);
        if !acceptable(w, release, &tokens, &want, &work_tokens, max_bytes) {
            continue;
        }
        let score = score(w, release, &tokens);
        let grabs = release.grabs.unwrap_or(0);
        let replace = match best {
            None => true,
            Some((best_score, best_grabs, best_size, best_guid, _)) => {
                score > best_score
                    || (score == best_score && grabs > best_grabs)
                    || (score == best_score && grabs == best_grabs && release.size < best_size)
                    || (score == best_score
                        && grabs == best_grabs
                        && release.size == best_size
                        && release.guid.as_str() < best_guid)
            }
        };
        if replace {
            best = Some((score, grabs, release.size, release.guid.as_str(), index));
        }
    }
    best.map(|(_, _, _, _, index)| &releases[index])
}

/// Inside a completed job folder the worker passes regular files. This skips a
/// symlink (it is not followed) and a name that is not `.epub`. Title-token
/// overlap wins, then the larger file.
pub(crate) fn choose_epub(w: &Wanted, files: &[(PathBuf, u64)]) -> Option<PathBuf> {
    let want = significant(w.title);
    let mut best: Option<(usize, u64, PathBuf)> = None;
    for (path, size) in files {
        if is_symlink(path) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".epub") {
            continue;
        }
        let overlap = want
            .iter()
            .filter(|token| tokens(name).contains(token))
            .count();
        let replace = match &best {
            None => true,
            Some((best_overlap, best_size, best_path)) => {
                overlap > *best_overlap
                    || (overlap == *best_overlap && size > best_size)
                    || (overlap == *best_overlap && size == best_size && path < best_path)
            }
        };
        if replace {
            best = Some((overlap, *size, path.clone()));
        }
    }
    best.map(|(_, _, path)| path)
}

/// Regular `.epub` files at depth ≤ 3. Symlinks are not followed.
pub(crate) fn list_epubs(dir: &std::path::Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    walk(dir, 1, &mut out);
    out
}

fn walk(dir: &std::path::Path, depth: u32, out: &mut Vec<(PathBuf, u64)>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if depth < 3 {
                walk(&path, depth + 1, out);
            }
        } else if meta.is_file() {
            let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("epub") {
                out.push((path, meta.len()));
            }
        }
    }
}

fn acceptable(
    w: &Wanted,
    release: &Release,
    tokens: &[String],
    want: &[String],
    work_tokens: &HashSet<String>,
    max_bytes: u64,
) -> bool {
    if !release.protocol.eq_ignore_ascii_case("usenet") {
        return false;
    }
    if release
        .download_url
        .as_deref()
        .is_none_or(|url| url.is_empty())
    {
        return false;
    }
    let cap = max_bytes.min(MAX_RELEASE);
    if release.size < MIN_RELEASE || release.size > cap {
        return false;
    }
    if want.is_empty()
        || want
            .iter()
            .any(|token| !tokens.iter().any(|have| have == token))
    {
        return false;
    }
    if !title_fits(w, &release.title) {
        return false;
    }
    let has_epub = tokens.iter().any(|token| token == "epub");
    if tokens
        .iter()
        .any(|token| BAD_FORMAT.contains(&token.as_str()))
        && !has_epub
    {
        return false;
    }
    if MARKERS
        .iter()
        .any(|marker| tokens.iter().any(|token| token == *marker) && !work_tokens.contains(*marker))
    {
        return false;
    }
    if has_phrase(tokens, "study", "guide")
        && !(work_tokens.contains("study") && work_tokens.contains("guide"))
    {
        return false;
    }
    true
}

fn score(w: &Wanted, release: &Release, tokens: &[String]) -> i64 {
    let mut score = 0i64;
    if tokens.iter().any(|token| token == "epub") {
        score += 50;
    }
    let surname = surname(w.author);
    if !surname.is_empty() && tokens.iter().any(|token| token == &surname) {
        score += 20;
    }
    if tokens.iter().any(|token| token == "retail") {
        score += 10;
    }
    if release
        .categories
        .iter()
        .any(|category| category.id == 7020)
    {
        score += 5;
    }
    if tokens
        .iter()
        .any(|token| LANGUAGES.contains(&token.as_str()))
    {
        score -= 15;
    }
    let grabs = release.grabs.unwrap_or(0).clamp(0, 20);
    score += grabs / 4;
    if release.size > BIG_RELEASE {
        score -= 10;
    }
    score
}

fn significant(title: &str) -> Vec<String> {
    let main = title.split(':').next().unwrap_or(title);
    tokens(main)
        .into_iter()
        .filter(|token| !STOP.contains(&token.as_str()))
        .collect()
}

struct Piece {
    words: Vec<String>,
    /// A series index with another title segment after it (`Dune 03 - Children`).
    /// The last numbered segment (`Dune 01`) is not a label; the index is stripped.
    label: bool,
}

/// Any ` - ` piece may be the book. A short index (`03`, `vol2`, `v2`) makes
/// that piece a series label only when a real title segment follows it.
/// Otherwise the index is dropped and the rest is matched. A catalog subtitle
/// matches a run of consecutive unlabeled pieces.
fn title_fits(w: &Wanted, release_title: &str) -> bool {
    let wanted_tokens: HashSet<String> = tokens(w.title).into_iter().collect();
    let author: HashSet<String> = tokens(w.author).into_iter().collect();
    let pending = release_parts(release_title)
        .into_iter()
        .filter_map(|words| {
            if words.is_empty() {
                return None;
            }
            let indexed = has_series_index(&words, &wanted_tokens);
            let words: Vec<String> = words
                .into_iter()
                .filter(|word| !dropped(word, &author, &wanted_tokens))
                .collect();
            if words.is_empty() {
                None
            } else {
                Some((words, indexed))
            }
        })
        .collect::<Vec<_>>();
    let title_after: Vec<bool> = pending
        .iter()
        .map(|(words, _)| words.iter().any(|word| title_word(word, &wanted_tokens)))
        .collect();
    let pieces = pending
        .into_iter()
        .enumerate()
        .filter_map(|(index, (words, indexed))| {
            let label = indexed && title_after[index + 1..].iter().any(|later| *later);
            let words = if label {
                words
            } else {
                words
                    .into_iter()
                    .filter(|word| !series_index(word) || wanted_tokens.contains(word))
                    .collect()
            };
            if words.is_empty() {
                None
            } else {
                Some(Piece { words, label })
            }
        })
        .collect::<Vec<_>>();
    let main = significant(w.title);
    if pieces
        .iter()
        .any(|piece| !piece.label && same_words(&piece.words, &main))
    {
        return true;
    }
    if w.title.contains(':') {
        return subtitle_run(&pieces, &tokens(w.title));
    }
    false
}

fn has_series_index(words: &[String], wanted: &HashSet<String>) -> bool {
    words
        .iter()
        .any(|word| !wanted.contains(word) && series_index(word))
}

fn title_word(word: &str, wanted: &HashSet<String>) -> bool {
    if wanted.contains(word) {
        return !STOP.contains(&word);
    }
    !(STOP.contains(&word)
        || EDITION.contains(&word)
        || is_format(word)
        || LANGUAGES.contains(&word)
        || is_year(word)
        || is_ordinal(word)
        || is_bare_count(word)
        || series_index(word))
}

/// `1`..`999`, or `vol`/`volume`/`book`/`part`/`v` glued to a number. A 4-digit
/// year is noise, not an index.
fn series_index(token: &str) -> bool {
    let digits = token.len();
    if (1..=3).contains(&digits) && token.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let rest = token
        .strip_prefix("volume")
        .or_else(|| token.strip_prefix("vol"))
        .or_else(|| token.strip_prefix("book"))
        .or_else(|| token.strip_prefix("part"))
        .or_else(|| token.strip_prefix('v'));
    rest.is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn subtitle_run(pieces: &[Piece], full: &[String]) -> bool {
    let mut run: Vec<&[String]> = Vec::new();
    for piece in pieces {
        if piece.label {
            if run_matches(&run, full) {
                return true;
            }
            run.clear();
            continue;
        }
        if !piece.words.is_empty() {
            run.push(&piece.words);
        }
    }
    run_matches(&run, full)
}

fn run_matches(run: &[&[String]], full: &[String]) -> bool {
    for start in 0..run.len() {
        for end in (start + 1)..=run.len() {
            let words: Vec<String> = run[start..end]
                .iter()
                .flat_map(|part| part.iter().cloned())
                .collect();
            if same_words(&words, full) {
                return true;
            }
        }
    }
    false
}

fn same_words(release: &[String], wanted: &[String]) -> bool {
    if wanted.is_empty() || wanted.iter().all(|word| STOP.contains(&word.as_str())) {
        return false;
    }
    let wanted_set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let release: Vec<&str> = release
        .iter()
        .map(String::as_str)
        .filter(|word| !ignorable(word, &wanted_set))
        .collect();
    let wanted: Vec<&str> = wanted
        .iter()
        .map(String::as_str)
        .filter(|word| !STOP.contains(word))
        .collect();
    !wanted.is_empty() && release == wanted
}

fn ignorable(word: &str, wanted: &HashSet<&str>) -> bool {
    if STOP.contains(&word) {
        return true;
    }
    EDITION.contains(&word) && !wanted.contains(word)
}

fn dropped(token: &str, author: &HashSet<String>, wanted: &HashSet<String>) -> bool {
    if wanted.contains(token) {
        return false;
    }
    author.contains(token)
        || is_format(token)
        || LANGUAGES.contains(&token)
        || is_year(token)
        || is_ordinal(token)
        || is_bare_count(token)
}

fn is_format(token: &str) -> bool {
    matches!(token, "epub" | "ebook" | "retail" | "azw" | "azw4" | "kf8")
        || BAD_FORMAT.contains(&token)
}

fn is_year(token: &str) -> bool {
    token.len() == 4 && token.chars().all(|c| c.is_ascii_digit())
}

fn is_ordinal(token: &str) -> bool {
    let digits = token.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == token.len() {
        return false;
    }
    matches!(&token[digits..], "st" | "nd" | "rd" | "th")
}

fn is_bare_count(token: &str) -> bool {
    matches!(token, "book" | "vol" | "volume" | "part")
}

/// Pieces of a release name split on a subtitle separator (`:` or ` - `,
/// including the `_` / `.` spellings scene names use).
fn release_parts(title: &str) -> Vec<Vec<String>> {
    let cleaned = strip_trailing_group(&strip_groups(title));
    let chars: Vec<char> = cleaned.chars().collect();
    let mut parts: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '_' && chars.get(i + 1) == Some(&'_') {
            push_word(&mut parts, &mut word);
            while chars.get(i) == Some(&'_') {
                i += 1;
            }
            parts.push(Vec::new());
            continue;
        }
        if c == ':' || separator_hyphen(&chars, i) {
            push_word(&mut parts, &mut word);
            parts.push(Vec::new());
            i += 1;
            continue;
        }
        if let Some(folded) = fold(c) {
            if folded == ' ' {
                push_word(&mut parts, &mut word);
            } else {
                word.push(folded);
            }
        }
        i += 1;
    }
    push_word(&mut parts, &mut word);
    parts
}

fn separator_hyphen(chars: &[char], index: usize) -> bool {
    if chars.get(index) != Some(&'-') {
        return false;
    }
    let prev = index == 0 || is_break(chars[index - 1]);
    let next = index + 1 == chars.len() || is_break(chars[index + 1]);
    prev && next
}

fn is_break(c: char) -> bool {
    c.is_whitespace() || c == '.' || c == '_'
}

fn push_word(parts: &mut [Vec<String>], word: &mut String) {
    if word.is_empty() {
        return;
    }
    parts
        .last_mut()
        .expect("release always has a part")
        .push(std::mem::take(word));
}

fn strip_groups(title: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in title.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        out.push(' ');
                    }
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A trailing `-GROUP` (`eBook-CTO`) is the release group, not the title.
fn strip_trailing_group(title: &str) -> String {
    let Some(dash) = title.rfind('-') else {
        return title.to_string();
    };
    if dash == 0 {
        return title.to_string();
    }
    let prev = title[..dash].chars().next_back();
    if prev.is_some_and(|c| c.is_whitespace() || c == '.' || c == '_') {
        return title.to_string();
    }
    let group = &title[dash + 1..];
    let group_ok = (2..=15).contains(&group.chars().count())
        && group
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && group.chars().any(|c| c.is_ascii_uppercase());
    if group_ok {
        title[..dash].to_string()
    } else {
        title.to_string()
    }
}

fn token_set(title: &str) -> HashSet<String> {
    tokens(title).into_iter().collect()
}

fn tokens(value: &str) -> Vec<String> {
    let folded: String = value.chars().filter_map(fold).collect();
    folded
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn surname(author: &str) -> String {
    tokens(author).pop().unwrap_or_default()
}

fn has_phrase(tokens: &[String], left: &str, right: &str) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == left && pair[1] == right)
}

fn is_symlink(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

fn fold(c: char) -> Option<char> {
    let kept = match c {
        'A'..='Z' => c.to_ascii_lowercase(),
        'a'..='z' | '0'..='9' => c,
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'À' | 'Á' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' => {
            'a'
        }
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'È' | 'É' | 'Ê' | 'Ë' | 'Ē' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' | 'Ø' | 'Ō' => {
            'o'
        }
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ý' | 'ÿ' | 'Ý' => 'y',
        'ß' => 's',
        '\'' | '\u{2019}' => return None,
        _ => ' ',
    };
    Some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(guid: &str, title: &str, size: u64, grabs: i64, categories: &[i64]) -> Release {
        Release {
            guid: guid.into(),
            indexer_id: 1,
            indexer: "NZBgeek".into(),
            title: title.into(),
            size,
            protocol: "usenet".into(),
            download_url: Some(format!("http://prowlarr/{guid}")),
            grabs: Some(grabs),
            seeders: None,
            categories: categories.iter().map(|id| Category { id: *id }).collect(),
        }
    }

    #[test]
    fn retail_epub_beats_pdf_and_german() {
        let wanted = Wanted {
            title: "The Teller of Small Fortunes",
            author: "Julie Leong",
        };
        let releases = [
            release(
                "pdf",
                "Julie.Leong.-.The.Teller.Of.Small.Fortunes.2024.PDF-CTO",
                2_000_000,
                50,
                &[7020],
            ),
            release(
                "de",
                "Julie.Leong.-.The.Teller.Of.Small.Fortunes.2024.German.EPUB-CTO",
                2_000_000,
                100,
                &[7020],
            ),
            release(
                "epub",
                "Julie.Leong.-.The.Teller.Of.Small.Fortunes.2024.RETAIL.EPUB.eBook-CTO",
                2_000_000,
                5,
                &[7020],
            ),
        ];
        let picked = pick(&wanted, &releases, &[], 64 * 1024 * 1024).unwrap();
        assert_eq!(picked.guid, "epub");
    }

    #[test]
    fn camus_stranger_matches_the_retail_name() {
        let wanted = Wanted {
            title: "The Stranger",
            author: "Albert Camus",
        };
        let releases = [release(
            "camus",
            "Albert_Camus_-_The_Stranger__The_Outsider___retail___epub",
            800_000,
            3,
            &[7020],
        )];
        assert_eq!(
            pick(&wanted, &releases, &[], 64 * 1024 * 1024)
                .unwrap()
                .guid,
            "camus"
        );
    }

    #[test]
    fn summary_of_dune_is_rejected() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let releases = [release("sum", "Summary of Dune EPUB", 800_000, 10, &[7020])];
        assert!(pick(&wanted, &releases, &[], 64 * 1024 * 1024).is_none());
    }

    #[test]
    fn the_release_title_is_the_book_not_a_sibling() {
        let max = 64 * 1024 * 1024;
        // (wanted title, author, release, accepted)
        let cases = [
            (
                "Dune",
                "Frank Herbert",
                "Frank Herbert - [Dune 06] - Chapterhouse Dune (retail) (epub)",
                false,
            ),
            (
                "Dune",
                "Frank Herbert",
                "Frank Herbert - Dune (retail) (epub)",
                true,
            ),
            ("Dune", "Frank Herbert", "Dune 01 (retail)", true),
            ("Dune", "Frank Herbert", "Frank Herbert - Dune 01", true),
            (
                "The Final Empire",
                "Brandon Sanderson",
                "Brandon Sanderson - The Final Empire 01",
                true,
            ),
            ("Title", "Ann Leckie", "Title v2", true),
            ("Dune", "Frank Herbert", "Dune Messiah", false),
            ("Dune", "Frank Herbert", "Children of Dune", false),
            ("Dune", "Frank Herbert", "Chapterhouse Dune", false),
            (
                "Sapiens",
                "Yuval Noah Harari",
                "Sapiens - A Brief History of Humankind",
                true,
            ),
            ("Dune", "Frank Herbert", "Dune: Deluxe Edition", true),
            (
                "Dune",
                "Frank Herbert",
                "The Dune Deluxe Illustrated Edition",
                true,
            ),
            (
                "Sapiens: A Brief History of Humankind",
                "Yuval Noah Harari",
                "Sapiens A Brief History of Humankind",
                true,
            ),
            (
                "Sapiens: A Brief History of Humankind",
                "Yuval Noah Harari",
                "Yuval Noah Harari - Sapiens: A Brief History of Humankind (epub)",
                true,
            ),
            (
                "Dune",
                "Frank Herbert",
                "Frank Herbert - Dune 03 - Children of Dune (epub)",
                false,
            ),
            (
                "Children of Dune",
                "Frank Herbert",
                "Frank Herbert - Dune 03 - Children of Dune (epub)",
                true,
            ),
            (
                "Dune",
                "Frank Herbert",
                "Frank Herbert - Dune 02 - Dune Messiah",
                false,
            ),
            (
                "Dune Messiah",
                "Frank Herbert",
                "Frank Herbert - Dune 02 - Dune Messiah",
                true,
            ),
            (
                "Pride and Prejudice",
                "Unknown",
                "Jane Austen - Pride and Prejudice (epub)",
                true,
            ),
            (
                "Good Omens",
                "Terry Pratchett",
                "Terry Pratchett & Neil Gaiman - Good Omens",
                true,
            ),
            (
                "Good Omens",
                "Terry Pratchett",
                "Neil Gaiman, Terry Pratchett - Good Omens",
                true,
            ),
            (
                "Crime and Punishment",
                "Fyodor Dostoyevsky",
                "Fyodor Dostoevsky - Crime and Punishment (epub)",
                true,
            ),
            (
                "The Hobbit",
                "J. R. R. Tolkien",
                "JRR Tolkien - The Hobbit (epub)",
                true,
            ),
            (
                "Solaris",
                "Stanisław Lem",
                "Stanislaw Lem - Solaris (epub)",
                true,
            ),
            (
                "Mistborn",
                "Brandon Sanderson",
                "Brandon Sanderson - Mistborn 01 - The Final Empire",
                false,
            ),
            (
                "The Final Empire",
                "Brandon Sanderson",
                "Brandon Sanderson - Mistborn 01 - The Final Empire",
                true,
            ),
            (
                "The Final Empire",
                "Brandon Sanderson",
                "Brandon Sanderson - Mistborn vol2 - The Final Empire",
                true,
            ),
            (
                "Harry Potter",
                "J.K. Rowling",
                "J.K. Rowling - Harry Potter 1 - Harry Potter and the Sorcerer's Stone",
                false,
            ),
            (
                "Harry Potter and the Sorcerer's Stone",
                "J.K. Rowling",
                "J.K. Rowling - Harry Potter 1 - Harry Potter and the Sorcerer's Stone",
                true,
            ),
            (
                "Remembrance of Earth's Past",
                "Cixin Liu",
                "Cixin Liu - Remembrance of Earth's Past 01 - The Three-Body Problem",
                false,
            ),
            (
                "The Three-Body Problem",
                "Cixin Liu",
                "Cixin Liu - Remembrance of Earth's Past 01 - The Three-Body Problem",
                true,
            ),
            (
                "Dune",
                "Frank Herbert",
                "Dune 40th Anniversary Edition",
                true,
            ),
            (
                "The Book Thief",
                "Markus Zusak",
                "Markus Zusak - The Book Thief (epub)",
                true,
            ),
            (
                "Dune",
                "Frank Herbert",
                "Frank Herbert - Dune vol (epub)",
                true,
            ),
            (
                "Sapiens: A Brief History of Humankind",
                "Yuval Noah Harari",
                "Notes - Sapiens Brief - History of Humankind - Appendix",
                true,
            ),
            (
                "Sapiens: A Brief History of Humankind",
                "Yuval Noah Harari",
                "Sapiens 01 - A Brief History of Humankind",
                false,
            ),
        ];
        for (title, author, name, accept) in cases {
            let wanted = Wanted { title, author };
            let releases = [release("row", name, 800_000, 5, &[7020])];
            let picked = pick(&wanted, &releases, &[], max);
            assert_eq!(picked.is_some(), accept, "{name}");
        }

        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let chapterhouse = release(
            "chapterhouse",
            "Frank Herbert - [Dune 06] - Chapterhouse Dune (retail) (epub)",
            800_000,
            40,
            &[7020],
        );
        let dune = release(
            "dune",
            "Frank Herbert - Dune (retail) (epub)",
            800_000,
            1,
            &[7020],
        );
        assert_eq!(
            pick(&wanted, &[chapterhouse, dune], &[], max).unwrap().guid,
            "dune"
        );
    }

    #[test]
    fn oversize_tried_and_ties() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let max = 64 * 1024 * 1024;
        let over = release(
            "big",
            "Frank.Herbert.Dune.RETAIL.EPUB",
            201 * 1024 * 1024,
            20,
            &[7020],
        );
        assert!(pick(&wanted, &[over], &[], max).is_none());
        let capped = release(
            "cap",
            "Frank.Herbert.Dune.RETAIL.EPUB",
            max + 1,
            20,
            &[7020],
        );
        assert!(pick(&wanted, &[capped], &[], max).is_none());

        let first = release("b", "Dune.Herbert.EPUB", 1_000_000, 4, &[7020]);
        let second = release("a", "Dune.Herbert.EPUB", 1_000_000, 4, &[7020]);
        let third = release("c", "Dune.Herbert.EPUB", 900_000, 4, &[7020]);
        let more = release("d", "Dune.Herbert.EPUB", 2_000_000, 8, &[7020]);
        assert_eq!(
            pick(&wanted, &[first.clone(), second.clone()], &[], max)
                .unwrap()
                .guid,
            "a"
        );
        assert_eq!(
            pick(&wanted, &[first.clone(), third.clone()], &[], max)
                .unwrap()
                .guid,
            "c"
        );
        assert_eq!(
            pick(&wanted, &[first.clone(), more.clone()], &[], max)
                .unwrap()
                .guid,
            "d"
        );
        assert_eq!(
            pick(&wanted, &[first, second], &["a".into()], max)
                .unwrap()
                .guid,
            "b"
        );
    }

    #[test]
    fn query_drops_the_subtitle() {
        assert_eq!(
            query(&Wanted {
                title: "Dune: The Butlerian Jihad",
                author: "Frank Herbert",
            }),
            "Dune Herbert"
        );
        assert_eq!(
            query(&Wanted {
                title: "The Teller of Small Fortunes",
                author: "Julie Leong",
            }),
            "The Teller of Small Fortunes Leong"
        );
        assert_eq!(
            query(&Wanted {
                title: "Dune",
                author: "Unknown",
            }),
            "Dune"
        );
        assert_eq!(
            query(&Wanted {
                title: "Dune",
                author: "unknown",
            }),
            "Dune"
        );
    }

    #[test]
    fn apostrophe_joins_the_word_and_a_title_without_latin_tokens_is_skipped() {
        let max = 200 * 1024 * 1024;
        let enders = release("e", "Orson.Scott.Card.Enders.Game.EPUB", 80_000, 1, &[7020]);
        assert_eq!(
            pick(
                &Wanted {
                    title: "Ender's Game",
                    author: "Orson Scott Card",
                },
                &[enders.clone()],
                &[],
                max,
            )
            .unwrap()
            .guid,
            "e"
        );
        assert_eq!(
            pick(
                &Wanted {
                    title: "Ender’s Game",
                    author: "Orson Scott Card",
                },
                &[enders],
                &[],
                max,
            )
            .unwrap()
            .guid,
            "e"
        );
        let cjk = release("c", "Liu.Cixin.Three.Body.EPUB", 80_000, 1, &[7020]);
        assert!(pick(
            &Wanted {
                title: "三体",
                author: "刘慈欣",
            },
            &[cjk],
            &[],
            max,
        )
        .is_none());
        let cyrillic = release(
            "k",
            "Dostoevsky.Prestuplenie.i.nakazanie.EPUB",
            80_000,
            1,
            &[7020],
        );
        assert!(pick(
            &Wanted {
                title: "Преступление и наказание",
                author: "Фёдор Достоевский",
            },
            &[cyrillic],
            &[],
            max,
        )
        .is_none());
    }

    #[test]
    fn choose_epub_overlap_extension_and_symlink() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("notes.pdf");
        std::fs::write(&other, b"x").unwrap();
        assert!(choose_epub(&wanted, &[(other, 10)]).is_none());

        let one = dir.path().join("Dune.epub");
        std::fs::write(&one, b"x").unwrap();
        assert_eq!(choose_epub(&wanted, &[(one.clone(), 10)]).unwrap(), one);

        let upper = dir.path().join("plain.EPUB");
        std::fs::write(&upper, b"xxxx").unwrap();
        let named = dir.path().join("Dune RETAIL.epub");
        std::fs::write(&named, b"x").unwrap();
        let picked = choose_epub(&wanted, &[(upper.clone(), 100), (named.clone(), 10)]).unwrap();
        assert_eq!(picked, named);

        let link = dir.path().join("bigger.epub");
        std::os::unix::fs::symlink(&upper, &link).unwrap();
        let picked = choose_epub(&wanted, &[(link, 999), (named.clone(), 10)]).unwrap();
        assert_eq!(picked, named);

        let nested = dir.path().join("mewo");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("Dune.epub"), b"nested").unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.epub"), b"secret").unwrap();
        std::os::unix::fs::symlink(&outside, nested.join("link")).unwrap();
        let listed = list_epubs(&nested);
        assert!(listed.iter().any(|(path, _)| path.ends_with("Dune.epub")));
        assert!(listed
            .iter()
            .all(|(path, _)| !path.ends_with("secret.epub")));
    }
}
