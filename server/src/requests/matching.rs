//! Which of an author's works explains a release name. No HTTP and no clock.
//!
//! A release is a bag of tokens. Author names, format tags, stopwords, and
//! numbers that are not part of any title are noise. The requested work has to
//! explain what remains better than any other work by the same author.

use crate::epub::OpfMeta;
use crate::requests::pick::fold;
use std::collections::HashSet;

#[derive(Clone)]
pub(crate) struct WorkContext {
    pub titles: Vec<String>,
    pub authors: Vec<String>,
    pub isbns: HashSet<String>,
    pub siblings: Vec<String>,
}

pub(crate) struct Matcher {
    wanted: Vec<Vec<String>>,
    siblings: Vec<(String, Vec<String>)>,
    authors: Vec<String>,
    keep_numbers: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    Match { fit: f32, author_seen: bool },
    NotTitle,
    Beaten { by: String },
}

/// `wanted_fit` is how well the requested work explains the text. `judge` drops
/// it; ranking a near-miss still needs it.
pub(crate) struct Scored {
    pub verdict: Verdict,
    pub wanted_fit: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Opf {
    Confirmed,
    Unknown,
    Mismatch(String),
}

const STOP: &[&str] = &[
    "a", "an", "the", "of", "and", "or", "to", "in", "on", "for", "with", "by", "from",
];
const TAGS: &[&str] = &[
    "epub",
    "kepub",
    "mobi",
    "azw3",
    "pdf",
    "ebook",
    "retail",
    "scan",
    "v1",
    "v2",
    "v3",
    "v4",
    "v5",
    "v6",
    "v7",
    "v8",
    "v9",
    "edition",
    "special",
    "deluxe",
    "anniversary",
    "unabridged",
    "illustrated",
    "annotated",
    "collectors",
    "collector",
    "book",
    "vol",
    "volume",
    "english",
    "eng",
];
const ROMAN: &[(&str, &str)] = &[
    ("i", "1"),
    ("ii", "2"),
    ("iii", "3"),
    ("iv", "4"),
    ("v", "5"),
    ("vi", "6"),
    ("vii", "7"),
    ("viii", "8"),
    ("ix", "9"),
    ("x", "10"),
    ("xi", "11"),
    ("xii", "12"),
    ("xiii", "13"),
    ("xiv", "14"),
    ("xv", "15"),
    ("xvi", "16"),
    ("xvii", "17"),
    ("xviii", "18"),
    ("xix", "19"),
    ("xx", "20"),
];

impl Matcher {
    pub(crate) fn new(ctx: &WorkContext) -> Self {
        let wanted: Vec<Vec<String>> = ctx
            .titles
            .iter()
            .map(|title| norm(title))
            .filter(|tokens| !tokens.is_empty())
            .collect();
        let wanted_sets: HashSet<Vec<String>> = wanted.iter().cloned().collect();
        let siblings = ctx
            .siblings
            .iter()
            .filter_map(|title| {
                let tokens = norm(title);
                if tokens.is_empty() || wanted_sets.contains(&tokens) {
                    None
                } else {
                    Some((title.clone(), tokens))
                }
            })
            .collect::<Vec<_>>();
        let mut authors = Vec::new();
        for author in &ctx.authors {
            for token in norm(author) {
                if !authors.iter().any(|have: &String| have == &token) {
                    authors.push(token);
                }
            }
        }
        let mut keep_numbers = HashSet::new();
        for tokens in wanted
            .iter()
            .chain(siblings.iter().map(|(_, tokens)| tokens))
        {
            for token in tokens {
                if digits(token) {
                    keep_numbers.insert(token.clone());
                }
            }
        }
        // A sibling that normalized to a wanted title was dropped above, but a
        // number that lives only on that duplicate still belongs to a title.
        for title in &ctx.siblings {
            for token in norm(title) {
                if digits(&token) {
                    keep_numbers.insert(token);
                }
            }
        }
        Self {
            wanted,
            siblings,
            authors,
            keep_numbers,
        }
    }

    pub(crate) fn judge(&self, text: &str) -> Verdict {
        self.score(text).verdict
    }

    pub(crate) fn score(&self, text: &str) -> Scored {
        let (release, author_seen) = self.clean(text);
        if self.wanted.is_empty() {
            return Scored {
                verdict: Verdict::NotTitle,
                wanted_fit: 0.0,
            };
        }
        let rlen = release.len() as u32;
        let mut best_hits = 0u32;
        let mut best_len = 1u32;
        let mut fit_hits = 0u32;
        let mut fit_len = 0u32;
        let mut have_fit = false;
        for option in &self.wanted {
            let hits = count_hits(option, &release);
            let len = option.len() as u32;
            if u64::from(hits) * u64::from(best_len) > u64::from(best_hits) * u64::from(len) {
                best_hits = hits;
                best_len = len;
            }
            let better_fit = !have_fit
                || u64::from(hits) * u64::from(fit_len + rlen)
                    > u64::from(fit_hits) * u64::from(len + rlen);
            if better_fit {
                fit_hits = hits;
                fit_len = len;
                have_fit = true;
            }
        }
        let wanted_fit = dice(fit_hits, fit_len, rlen);
        if u64::from(best_hits) * 5 < u64::from(best_len) * 4 {
            return Scored {
                verdict: Verdict::NotTitle,
                wanted_fit,
            };
        }
        let mut beaten: Option<(String, u32, u32)> = None;
        for (title, tokens) in &self.siblings {
            let hits = count_hits(tokens, &release);
            let len = tokens.len() as u32;
            if rlen == 0 || len == 0 {
                continue;
            }
            if u64::from(hits) * u64::from(fit_len + rlen)
                <= u64::from(fit_hits) * u64::from(len + rlen)
            {
                continue;
            }
            let replace = match &beaten {
                None => true,
                Some((_, best_hits, best_len)) => {
                    u64::from(hits) * u64::from(best_len + rlen)
                        > u64::from(*best_hits) * u64::from(len + rlen)
                }
            };
            if replace {
                beaten = Some((title.clone(), hits, len));
            }
        }
        if let Some((by, _, _)) = beaten {
            return Scored {
                verdict: Verdict::Beaten { by },
                wanted_fit,
            };
        }
        Scored {
            verdict: Verdict::Match {
                fit: wanted_fit,
                author_seen,
            },
            wanted_fit,
        }
    }

    pub(crate) fn has_token(&self, token: &str) -> bool {
        self.wanted
            .iter()
            .any(|option| option.iter().any(|have| have == token))
    }

    fn clean(&self, text: &str) -> (Vec<String>, bool) {
        let tokens = norm(text);
        let author_seen = tokens
            .iter()
            .any(|token| self.authors.iter().any(|author| eq(token, author)));
        let release = tokens
            .into_iter()
            .filter(|token| {
                if digits(token) && !self.keep_numbers.contains(token) {
                    return false;
                }
                !self.authors.iter().any(|author| eq(token, author))
            })
            .collect();
        (release, author_seen)
    }
}

pub(crate) fn verify_opf(matcher: &Matcher, ctx: &WorkContext, meta: &OpfMeta) -> Opf {
    let confirmed = meta
        .identifiers
        .iter()
        .any(|raw| isbn13s(raw).iter().any(|isbn| ctx.isbns.contains(isbn)));
    if confirmed {
        return Opf::Confirmed;
    }
    if let Some(language) = meta
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !language.to_ascii_lowercase().starts_with("en") {
            return Opf::Mismatch(format!("Downloaded book is in {language}"));
        }
    }
    let mut text = String::new();
    for part in meta.titles.iter().chain(meta.creators.iter()) {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(part);
    }
    match matcher.judge(&text) {
        Verdict::Beaten { by } => Opf::Mismatch(format!("Downloaded book is {by}")),
        Verdict::NotTitle => Opf::Unknown,
        Verdict::Match { .. } => Opf::Confirmed,
    }
}

/// ASCII-fold, split on non-alphanumerics, drop stopwords and edition tags,
/// drop ordinals (`40th`), fold roman numerals i–xx, and dedupe in order.
pub(crate) fn norm(text: &str) -> Vec<String> {
    let folded: String = text.chars().filter_map(fold).collect();
    let mut out = Vec::new();
    for token in folded.split_whitespace() {
        if token.is_empty() || STOP.contains(&token) || TAGS.contains(&token) || ordinal(token) {
            continue;
        }
        let token = fold_roman(token);
        if out.iter().any(|have: &String| have == &token) {
            continue;
        }
        out.push(token);
    }
    out
}

/// Every ISBN-10 or ISBN-13 in `raw`, as ISBN-13. Invalid check digits are
/// ignored. Hyphens, spaces, and a `urn:isbn:` prefix are fine.
pub(crate) fn isbn13s(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut body = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() {
                body.push(c);
                i += 1;
            } else if c == '-' || c == ' ' || c == '\u{00a0}' {
                i += 1;
            } else if (c == 'X' || c == 'x') && body.len() == 9 {
                body.push('X');
                i += 1;
                break;
            } else {
                break;
            }
        }
        if let Some(isbn) = canonical_isbn(&body) {
            if !out.contains(&isbn) {
                out.push(isbn);
            }
        }
        if i == start {
            i += 1;
        }
    }
    out
}

fn canonical_isbn(body: &str) -> Option<String> {
    if body.len() == 13 && body.bytes().all(|b| b.is_ascii_digit()) {
        let prefix = &body[..3];
        if (prefix == "978" || prefix == "979") && isbn13_ok(body) {
            return Some(body.to_string());
        }
        return None;
    }
    if body.len() == 10 && isbn10_ok(body) {
        return Some(isbn10_to_13(body));
    }
    None
}

fn isbn10_ok(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() != 10 {
        return false;
    }
    let mut sum = 0u32;
    for (index, c) in chars.iter().enumerate() {
        let value = match c {
            '0'..='9' => *c as u32 - '0' as u32,
            'X' | 'x' if index == 9 => 10,
            _ => return false,
        };
        sum += value * (10 - index as u32);
    }
    sum % 11 == 0
}

fn isbn13_ok(body: &str) -> bool {
    let mut sum = 0u32;
    for (index, c) in body.chars().enumerate() {
        let Some(value) = c.to_digit(10) else {
            return false;
        };
        sum += value * if index % 2 == 0 { 1 } else { 3 };
    }
    sum % 10 == 0
}

fn isbn10_to_13(body: &str) -> String {
    let mut out = String::from("978");
    out.push_str(&body[..9]);
    let mut sum = 0u32;
    for (index, c) in out.chars().enumerate() {
        let value = c.to_digit(10).unwrap_or(0);
        sum += value * if index % 2 == 0 { 1 } else { 3 };
    }
    let check = (10 - (sum % 10)) % 10;
    out.push(char::from_digit(check, 10).unwrap_or('0'));
    out
}

fn count_hits(left: &[String], right: &[String]) -> u32 {
    left.iter()
        .filter(|token| right.iter().any(|have| eq(token, have)))
        .count() as u32
}

fn dice(hits: u32, left: u32, right: u32) -> f32 {
    if left == 0 || right == 0 {
        0.0
    } else {
        2.0 * hits as f32 / (left + right) as f32
    }
}

fn eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let n = a.chars().count().min(b.chars().count());
    if n < 5 {
        return false;
    }
    let limit = if n >= 8 { 2 } else { 1 };
    levenshtein(a, b) <= limit
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn digits(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit())
}

fn ordinal(token: &str) -> bool {
    let rest = token
        .strip_suffix("th")
        .or_else(|| token.strip_suffix("st"))
        .or_else(|| token.strip_suffix("nd"))
        .or_else(|| token.strip_suffix("rd"));
    matches!(
        rest,
        Some(rest) if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
    )
}

fn fold_roman(token: &str) -> String {
    ROMAN
        .iter()
        .find(|(roman, _)| *roman == token)
        .map(|(_, arabic)| (*arabic).to_string())
        .unwrap_or_else(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::{self, OpfMeta};
    use crate::test_epub;
    use std::io::Cursor;

    fn ctx(title: &str, authors: &[&str], siblings: Vec<String>) -> WorkContext {
        WorkContext {
            titles: vec![title.to_string()],
            authors: authors.iter().map(|author| (*author).to_string()).collect(),
            isbns: HashSet::new(),
            siblings,
        }
    }

    fn titles(name: &str) -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ol")
            .join(name);
        let text = std::fs::read_to_string(path).unwrap();
        #[derive(serde::Deserialize)]
        struct Doc {
            title: String,
        }
        #[derive(serde::Deserialize)]
        struct Page {
            docs: Vec<Doc>,
        }
        let page: Page = serde_json::from_str(&text).unwrap();
        page.docs.into_iter().map(|doc| doc.title).collect()
    }

    fn near(fit: f32, expected: f32) -> bool {
        (fit - expected).abs() < 0.001
    }

    #[test]
    fn failure_corpus_matches_the_author_s_other_works() {
        let herbert = titles("herbert.json");
        let sanderson = titles("sanderson.json");
        let orwell = titles("orwell.json");
        let cases = [
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - [Dune 06] - Chapterhouse Dune (retail) (epub)",
                Kind::Beaten("Chapterhouse Dune"),
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune 06 - Chapterhouse Dune (retail) (epub)",
                Kind::Beaten("Chapterhouse Dune"),
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune 01 - Special Edition (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank.Herbert.-.Dune.01.(epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune 03 - Children of Dune",
                Kind::Beaten("Children of Dune"),
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune (40th Anniversary Edition) (retail) (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Herbert, Frank - Dune: Deluxe Edition (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune Messiah (epub)",
                Kind::Beaten("Dune Messiah"),
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune Saga 01 - Dune (epub)",
                Kind::Match,
                2.0 / 3.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune Complete Series (epub)",
                Kind::Beaten("Dune Complete Series, 6 Books"),
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Herbert_Frank-Dune-1965-RETAIL-EPUB-eBook-CTO",
                Kind::Match,
                2.0 / 3.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Der Wustenplanet (epub)",
                Kind::NotTitle,
                0.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Dune (SparkNotes Literature Guide) epub",
                Kind::Match,
                0.4,
                false,
            ),
            (
                "Dune Messiah",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune 02 - Dune Messiah (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Final Empire",
                &["Brandon Sanderson"][..],
                "sanderson",
                "Brandon Sanderson - The Final Empire 01 - Mistborn (epub)",
                Kind::Match,
                0.8,
                true,
            ),
            (
                "The Final Empire",
                &["Brandon Sanderson"][..],
                "sanderson",
                "Brandon Sanderson - Mistborn 01 - The Final Empire (retail) (epub)",
                Kind::Match,
                0.8,
                true,
            ),
            (
                "The Final Empire",
                &["Brandon Sanderson"][..],
                "sanderson",
                "Brandon Sanderson - Mistborn 02 - The Well of Ascension (epub)",
                Kind::NotTitle,
                0.0,
                false,
            ),
            (
                "The Final Empire",
                &["Brandon Sanderson"][..],
                "sanderson",
                "Sanderson, Brandon - Mistborn - The Final Empire (epub)",
                Kind::Match,
                0.8,
                true,
            ),
            (
                "The Final Empire",
                &["Brandon Sanderson"][..],
                "sanderson",
                "Brandon Sanderson - Mistborn Trilogy (epub)",
                Kind::NotTitle,
                0.0,
                false,
            ),
            (
                "Good Omens",
                &["Neil Gaiman", "Terry Pratchett"][..],
                "",
                "Terry Pratchett & Neil Gaiman - Good Omens (retail) (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Good Omens",
                &["Neil Gaiman"][..],
                "",
                "Terry Pratchett & Neil Gaiman - Good Omens (retail) (epub)",
                Kind::Match,
                2.0 / 3.0,
                true,
            ),
            (
                "Crime and Punishment",
                &["Fyodor Dostoevsky"][..],
                "",
                "Fyodor Dostoyevsky - Crime and Punishment (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "1984",
                &["George Orwell"][..],
                "orwell",
                "George Orwell - 1984 (retail) (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "1984",
                &["George Orwell"][..],
                "orwell",
                "George Orwell - Animal Farm 1945 (epub)",
                Kind::NotTitle,
                0.0,
                false,
            ),
            (
                "The Old Man and the Sea",
                &["Ernest Hemingway"][..],
                "hemingway",
                "Ernest Hemingway - The Old Man and the Sea (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Old Man and the Sea",
                &["Ernest Hemingway"][..],
                "hemingway",
                "Ernest Hemingway - The Old Man (epub)",
                Kind::NotTitle,
                0.0,
                false,
            ),
            (
                "The Teller of Small Fortunes",
                &["Julie Leong"][..],
                "",
                "Julie.Leong.-.The.Teller.Of.Small.Fortunes.2024.RETAIL.EPUB.eBook-CTO",
                Kind::Match,
                6.0 / 7.0,
                true,
            ),
            (
                "The Stranger",
                &["Albert Camus"][..],
                "",
                "Albert_Camus_-_The_Stranger__The_Outsider___retail___epub",
                Kind::Match,
                2.0 / 3.0,
                true,
            ),
        ];
        for (title, authors, fixture, release, kind, fit, author_seen) in cases {
            let siblings = match fixture {
                "herbert" => herbert.clone(),
                "sanderson" => sanderson.clone(),
                "orwell" => orwell.clone(),
                "hemingway" => vec!["The Sun Also Rises".into()],
                _ => Vec::new(),
            };
            let verdict = Matcher::new(&ctx(title, authors, siblings)).judge(release);
            match (kind, verdict) {
                (
                    Kind::Match,
                    Verdict::Match {
                        fit: got,
                        author_seen: seen,
                    },
                ) => {
                    assert!(near(got, fit), "{title} | {release} fit {got} != {fit}");
                    assert_eq!(seen, author_seen, "{title} | {release}");
                }
                (Kind::NotTitle, Verdict::NotTitle) => {}
                (Kind::Beaten(by), Verdict::Beaten { by: got }) => {
                    assert_eq!(got, by, "{title} | {release}");
                }
                (_, other) => panic!("{title} | {release} -> {other:?}, expected {kind:?}"),
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Kind {
        Match,
        NotTitle,
        Beaten(&'static str),
    }

    #[test]
    fn tags_and_author_tokens_do_not_change_the_verdict() {
        let matcher = Matcher::new(&ctx(
            "Dune",
            &["Frank Herbert"],
            vec!["Chapterhouse Dune".into(), "Dune Messiah".into()],
        ));
        let samples = [
            "Frank Herbert - Dune 01 - Special Edition",
            "Frank Herbert - [Dune 06] - Chapterhouse Dune",
            "Der Wustenplanet",
        ];
        for sample in samples {
            let original = matcher.judge(sample);
            for tag in ["retail", "epub", "scan", "deluxe", "v3", "40th", "edition"] {
                let tagged = format!("{sample} {tag}");
                assert_eq!(matcher.judge(&tagged), original, "{tagged}");
            }
            let with_author = format!("{sample} Herbert");
            assert_eq!(matcher.judge(&with_author), original, "{with_author}");
        }
        let already = matcher.judge("Frank Herbert - Dune");
        assert_eq!(matcher.judge("Frank Herbert - Dune Herbert"), already);
    }

    #[test]
    fn a_sibling_equal_to_the_wanted_title_never_beats_it() {
        let plain = ctx("Dune", &["Frank Herbert"], vec!["Chapterhouse Dune".into()]);
        let duplicated = ctx(
            "Dune",
            &["Frank Herbert"],
            vec![
                "Dune".into(),
                "DUNE".into(),
                "Dune ".into(),
                "Chapterhouse Dune".into(),
            ],
        );
        let matcher = Matcher::new(&plain);
        let extra = Matcher::new(&duplicated);
        for release in [
            "Frank Herbert - Dune (epub)",
            "Frank Herbert - Chapterhouse Dune (epub)",
            "Frank Herbert - Dune Messiah (epub)",
        ] {
            assert_eq!(extra.judge(release), matcher.judge(release), "{release}");
        }
        assert!(matches!(
            extra.judge("Dune RETAIL EPUB"),
            Verdict::Match { fit, .. } if near(fit, 1.0)
        ));
    }

    #[test]
    fn roman_numerals_fold_and_stray_numbers_drop() {
        let matcher = Matcher::new(&ctx("Dune", &["Frank Herbert"], vec![]));
        assert_eq!(norm("Dune II"), vec!["dune".to_string(), "2".to_string()]);
        assert_eq!(matcher.judge("Dune II"), matcher.judge("Dune"));
        assert_eq!(norm("colour"), vec!["colour".to_string()]);
        assert!(eq("colour", "color"));
        assert!(eq("dostoevsky", "dostoyevsky"));
        assert!(!eq("dune", "done"));
    }

    #[test]
    fn isbn10_converts_and_check_digits_are_required() {
        assert_eq!(isbn13s("0441172717"), vec!["9780441172719".to_string()]);
        assert_eq!(isbn13s("9780441172719"), vec!["9780441172719".to_string()]);
        assert_eq!(
            isbn13s("urn:isbn:0-441-17271-7"),
            vec!["9780441172719".to_string()]
        );
        assert!(isbn13s("0441172718").is_empty());
        assert!(isbn13s("9780441172718").is_empty());
    }

    fn dune_meta(isbn: Option<&str>) -> WorkContext {
        let mut context = ctx(
            "Dune",
            &["Frank Herbert"],
            vec!["Chapterhouse Dune".into(), "Dune".into()],
        );
        if let Some(isbn) = isbn {
            context.isbns.extend(isbn13s(isbn));
        }
        context
    }

    fn parsed(metadata: &str) -> epub::ParsedEpub {
        epub::parse(Cursor::new(test_epub::with_metadata(metadata)), 1024).unwrap()
    }

    #[test]
    fn opf_isbn_confirms_a_sibling_title() {
        let context = dune_meta(Some("9780441172719"));
        let matcher = Matcher::new(&context);
        let book = parsed(
            "<dc:title>Chapterhouse Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:identifier>urn:isbn:978-0-441-17271-9</dc:identifier><dc:language>de</dc:language>",
        );
        assert_eq!(book.meta.identifiers.len(), 1);
        assert_eq!(book.meta.language.as_deref(), Some("de"));
        assert_eq!(verify_opf(&matcher, &context, &book.meta), Opf::Confirmed);
    }

    #[test]
    fn opf_sibling_title_language_and_junk_title() {
        let context = dune_meta(None);
        let matcher = Matcher::new(&context);
        let sibling =
            parsed("<dc:title>Chapterhouse Dune</dc:title><dc:creator>Frank Herbert</dc:creator>");
        assert_eq!(
            verify_opf(&matcher, &context, &sibling.meta),
            Opf::Mismatch("Downloaded book is Chapterhouse Dune".into())
        );
        let junk =
            parsed("<dc:title>Microsoft Word - doc1</dc:title><dc:creator>User</dc:creator>");
        assert_eq!(junk.meta.titles, vec!["Microsoft Word - doc1".to_string()]);
        assert_eq!(verify_opf(&matcher, &context, &junk.meta), Opf::Unknown);
        let german = parsed(
            "<dc:title>Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:language>de</dc:language>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &german.meta),
            Opf::Mismatch("Downloaded book is in de".into())
        );
        let english = parsed(
            "<dc:title>Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:language>en-US</dc:language>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &english.meta),
            Opf::Confirmed
        );
    }

    #[test]
    fn direct_opf_meta_without_a_file_follows_the_same_order() {
        let context = dune_meta(Some("0441172717"));
        let matcher = Matcher::new(&context);
        let meta = OpfMeta {
            titles: vec!["Something else".into()],
            creators: vec![],
            identifiers: vec!["0-441-17271-7".into()],
            language: Some("de".into()),
        };
        assert_eq!(verify_opf(&matcher, &context, &meta), Opf::Confirmed);
    }
}
