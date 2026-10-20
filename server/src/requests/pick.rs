//! Release gates and ranking. No HTTP and no clock. Title fit lives in
//! [`super::matching`].

use super::matching::{Matcher, Verdict};
use serde::Deserialize;
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
    #[allow(dead_code)]
    #[serde(default)]
    pub categories: Vec<Category>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Category {
    pub id: i64,
}

const MIN_RELEASE: u64 = 50 * 1024;
const MAX_RELEASE: u64 = 200 * 1024 * 1024;

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
const MARKERS: &[&str] = &[
    "summary",
    "workbook",
    "sampler",
    "sparknotes",
    "cliffsnotes",
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

pub(crate) struct Choice<'a> {
    pub release: Option<&'a Release>,
    /// Set when nothing was acceptable and some release lost to another work.
    pub closest: Option<String>,
}

/// Best release not in `tried`. `closest` is the sibling that beat the nearest
/// miss, for the error text.
pub(crate) fn pick<'r>(
    matcher: &Matcher,
    releases: &'r [Release],
    tried: &[String],
    max_bytes: u64,
    languages: &[String],
) -> Choice<'r> {
    let mut best: Option<Rank<'r>> = None;
    let mut closest: Option<(f32, String)> = None;
    for release in releases {
        if tried.iter().any(|guid| guid == &release.guid) {
            continue;
        }
        let tokens = tokens(&release.title);
        if !gates(release, &tokens, matcher, max_bytes) {
            continue;
        }
        let scored = matcher.score(&release.title);
        match scored.verdict {
            Verdict::Match { fit, author_seen } => {
                let rank = Rank {
                    author_seen,
                    not_foreign: !foreign(&tokens, languages),
                    retail: tokens.iter().any(|token| token == "retail"),
                    fit,
                    grabs: release.grabs.unwrap_or(0).min(20),
                    size: release.size,
                    guid: release.guid.as_str(),
                    release,
                };
                if best.as_ref().is_none_or(|have| rank.beats(have)) {
                    best = Some(rank);
                }
            }
            Verdict::Beaten { by } => {
                if closest
                    .as_ref()
                    .is_none_or(|(fit, _)| scored.wanted_fit > *fit)
                {
                    closest = Some((scored.wanted_fit, by));
                }
            }
            Verdict::NotTitle => {}
        }
    }
    Choice {
        closest: if best.is_some() {
            None
        } else {
            closest.map(|(_, by)| by)
        },
        release: best.map(|rank| rank.release),
    }
}

struct Rank<'a> {
    author_seen: bool,
    not_foreign: bool,
    retail: bool,
    fit: f32,
    grabs: i64,
    size: u64,
    guid: &'a str,
    release: &'a Release,
}

impl Rank<'_> {
    fn beats(&self, other: &Rank<'_>) -> bool {
        self.author_seen
            .cmp(&other.author_seen)
            .then(self.not_foreign.cmp(&other.not_foreign))
            .then(self.retail.cmp(&other.retail))
            .then(self.fit.total_cmp(&other.fit))
            .then(self.grabs.cmp(&other.grabs))
            .then(other.size.cmp(&self.size))
            .then(other.guid.cmp(self.guid))
            .is_gt()
    }
}

/// Inside a completed job folder the worker passes regular files. This skips a
/// symlink (it is not followed) and a name that is not `.epub`. A name the
/// matcher accepts wins on fit, then the larger file.
pub(crate) fn choose_epub(matcher: &Matcher, files: &[(PathBuf, u64)]) -> Option<PathBuf> {
    let mut best: Option<(f32, u64, PathBuf)> = None;
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
        let fit = match matcher.judge(name) {
            Verdict::Match { fit, .. } => fit,
            Verdict::NotTitle | Verdict::Beaten { .. } => 0.0,
        };
        let replace = match &best {
            None => true,
            Some((best_fit, best_size, best_path)) => {
                fit > *best_fit
                    || (fit == *best_fit && size > best_size)
                    || (fit == *best_fit && size == best_size && path < best_path)
            }
        };
        if replace {
            best = Some((fit, *size, path.clone()));
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

fn gates(release: &Release, tokens: &[String], matcher: &Matcher, max_bytes: u64) -> bool {
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
        .any(|marker| tokens.iter().any(|token| token == *marker) && !matcher.has_token(marker))
    {
        return false;
    }
    let study_guide = tokens
        .windows(2)
        .any(|pair| pair[0] == "study" && pair[1] == "guide");
    if study_guide && !(matcher.has_token("study") && matcher.has_token("guide")) {
        return false;
    }
    true
}

fn foreign(tokens: &[String], languages: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| super::languages::foreign_word(token, languages))
}

fn tokens(value: &str) -> Vec<String> {
    let folded: String = value.chars().filter_map(fold).collect();
    folded
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_symlink(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

pub(crate) fn fold(c: char) -> Option<char> {
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
    use crate::requests::matching::WorkContext;
    use std::collections::HashSet;

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().copied().map(str::to_string).collect()
    }

    fn matcher_for(w: &Wanted) -> Matcher {
        Matcher::new(&WorkContext {
            titles: vec![w.title.to_string()],
            authors: vec![w.author.to_string()],
            isbns: HashSet::new(),
            siblings: Vec::new(),
        })
    }

    fn pick_of<'a>(
        w: &Wanted,
        releases: &'a [Release],
        tried: &[String],
        max: u64,
    ) -> Option<&'a Release> {
        pick(&matcher_for(w), releases, tried, max, &codes(&["en"])).release
    }

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
        let picked = pick_of(&wanted, &releases, &[], 64 * 1024 * 1024).unwrap();
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
            pick_of(&wanted, &releases, &[], 64 * 1024 * 1024)
                .unwrap()
                .guid,
            "camus"
        );
    }

    #[test]
    fn chapterhouse_loses_to_the_sibling_and_guides_are_rejected() {
        let max = 64 * 1024 * 1024;
        let matcher = Matcher::new(&WorkContext {
            titles: vec!["Dune".into()],
            authors: vec!["Frank Herbert".into()],
            isbns: HashSet::new(),
            siblings: vec!["Dune".into(), "Chapterhouse Dune".into()],
        });
        let chapterhouse = release(
            "ch",
            "Frank Herbert - [Dune 06] - Chapterhouse Dune (retail) (epub)",
            800_000,
            10,
            &[7020],
        );
        let releases = [chapterhouse];
        let choice = pick(&matcher, &releases, &[], max, &codes(&["en"]));
        assert!(choice.release.is_none());
        assert_eq!(choice.closest.as_deref(), Some("Chapterhouse Dune"));

        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let spark = release(
            "spark",
            "Dune (SparkNotes Literature Guide) epub",
            800_000,
            10,
            &[7020],
        );
        let study = release("study", "Dune Study Guide epub", 800_000, 10, &[7020]);
        let cliffs = release("cliffs", "Dune CliffsNotes epub", 800_000, 10, &[7020]);
        assert!(pick_of(&wanted, &[spark, study, cliffs], &[], max).is_none());
    }

    #[test]
    fn not_foreign_beats_a_retail_release_and_fit_beats_grabs() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let max = 64 * 1024 * 1024;
        let english = release("en", "Frank.Herbert.Dune.Saga.EPUB", 2_000_000, 0, &[7020]);
        let german = release(
            "de",
            "Frank.Herbert.Dune.German.RETAIL.EPUB",
            800_000,
            20,
            &[7020],
        );
        assert_eq!(
            pick_of(&wanted, &[german, english], &[], max).unwrap().guid,
            "en"
        );
        let exact = release("exact", "Frank.Herbert.Dune.EPUB", 2_000_000, 0, &[7020]);
        let saga = release("saga", "Frank.Herbert.Dune.Saga.EPUB", 800_000, 20, &[7020]);
        assert_eq!(
            pick_of(&wanted, &[saga, exact], &[], max).unwrap().guid,
            "exact"
        );
    }

    #[test]
    fn an_accepted_language_is_not_ranked_foreign() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let max = 64 * 1024 * 1024;
        let english = release("en", "Frank.Herbert.Dune.Saga.EPUB", 2_000_000, 0, &[7020]);
        let german = release(
            "de",
            "Frank.Herbert.Dune.German.RETAIL.EPUB",
            800_000,
            20,
            &[7020],
        );
        let matcher = matcher_for(&wanted);
        let both = codes(&["en", "de"]);
        assert_eq!(
            pick(
                &matcher,
                &[german.clone(), english.clone()],
                &[],
                max,
                &both
            )
            .release
            .unwrap()
            .guid,
            "de"
        );
        let only = codes(&["en"]);
        assert_eq!(
            pick(&matcher, &[german, english], &[], max, &only)
                .release
                .unwrap()
                .guid,
            "en"
        );
    }

    #[test]
    fn author_seen_outranks_retail_and_grabs_cap_at_twenty() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let max = 64 * 1024 * 1024;
        let named = release("named", "Frank.Herbert.Dune.EPUB", 2_000_000, 0, &[7020]);
        let retail = release("retail", "Dune.RETAIL.EPUB", 800_000, 20, &[7020]);
        assert_eq!(
            pick_of(&wanted, &[retail, named], &[], max).unwrap().guid,
            "named"
        );
        let many = release("many", "Frank.Herbert.Dune.EPUB", 2_000_000, 100, &[7020]);
        let fewer = release("fewer", "Frank.Herbert.Dune.EPUB", 800_000, 50, &[7020]);
        assert_eq!(
            pick_of(&wanted, &[many, fewer], &[], max).unwrap().guid,
            "fewer"
        );
    }

    #[test]
    fn summary_of_dune_is_rejected() {
        let wanted = Wanted {
            title: "Dune",
            author: "Frank Herbert",
        };
        let releases = [release("sum", "Summary of Dune EPUB", 800_000, 10, &[7020])];
        assert!(pick_of(&wanted, &releases, &[], 64 * 1024 * 1024).is_none());
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
        assert!(pick_of(&wanted, &[over], &[], max).is_none());
        let capped = release(
            "cap",
            "Frank.Herbert.Dune.RETAIL.EPUB",
            max + 1,
            20,
            &[7020],
        );
        assert!(pick_of(&wanted, &[capped], &[], max).is_none());

        let first = release("b", "Dune.Herbert.EPUB", 1_000_000, 4, &[7020]);
        let second = release("a", "Dune.Herbert.EPUB", 1_000_000, 4, &[7020]);
        let third = release("c", "Dune.Herbert.EPUB", 900_000, 4, &[7020]);
        let more = release("d", "Dune.Herbert.EPUB", 2_000_000, 8, &[7020]);
        assert_eq!(
            pick_of(&wanted, &[first.clone(), second.clone()], &[], max)
                .unwrap()
                .guid,
            "a"
        );
        assert_eq!(
            pick_of(&wanted, &[first.clone(), third.clone()], &[], max)
                .unwrap()
                .guid,
            "c"
        );
        assert_eq!(
            pick_of(&wanted, &[first.clone(), more.clone()], &[], max)
                .unwrap()
                .guid,
            "d"
        );
        assert_eq!(
            pick_of(&wanted, &[first, second], &["a".into()], max)
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
            pick_of(
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
            pick_of(
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
        assert!(pick_of(
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
        assert!(pick_of(
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
        assert!(choose_epub(&matcher_for(&wanted), &[(other, 10)]).is_none());

        let one = dir.path().join("Dune.epub");
        std::fs::write(&one, b"x").unwrap();
        assert_eq!(
            choose_epub(&matcher_for(&wanted), &[(one.clone(), 10)]).unwrap(),
            one
        );

        let upper = dir.path().join("plain.EPUB");
        std::fs::write(&upper, b"xxxx").unwrap();
        let named = dir.path().join("Dune RETAIL.epub");
        std::fs::write(&named, b"x").unwrap();
        let picked = choose_epub(
            &matcher_for(&wanted),
            &[(upper.clone(), 100), (named.clone(), 10)],
        )
        .unwrap();
        assert_eq!(picked, named);

        let link = dir.path().join("bigger.epub");
        std::os::unix::fs::symlink(&upper, &link).unwrap();
        let picked =
            choose_epub(&matcher_for(&wanted), &[(link, 999), (named.clone(), 10)]).unwrap();
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
