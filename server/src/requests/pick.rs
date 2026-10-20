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

/// The search string sent to Prowlarr: `<title without subtitle> <author surname>`.
pub(crate) fn query(w: &Wanted) -> String {
    let title = w.title.split(':').next().unwrap_or(w.title);
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let surname = w.author.split_whitespace().last().unwrap_or("");
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
    let work_tokens = token_set(w.title);
    let mut best: Option<(i64, i64, u64, &str, usize)> = None;
    for (index, release) in releases.iter().enumerate() {
        if tried.iter().any(|guid| guid == &release.guid) {
            continue;
        }
        let tokens = tokens(&release.title);
        if !acceptable(release, &tokens, &want, &work_tokens, max_bytes) {
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
    if !want.is_empty()
        && want
            .iter()
            .any(|token| !tokens.iter().any(|have| have == token))
    {
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

fn token_set(title: &str) -> HashSet<String> {
    tokens(title).into_iter().collect()
}

fn tokens(value: &str) -> Vec<String> {
    let folded: String = value.chars().map(fold).collect();
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

fn fold(c: char) -> char {
    match c {
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
        _ => ' ',
    }
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
