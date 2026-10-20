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
    "movie",
    "tie",
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
        // A pre-colon fragment that is another of the author's books is not a
        // name for this work. "Mistborn: Secret History" must not claim the
        // sibling "Mistborn".
        let titles: Vec<&String> = ctx
            .titles
            .iter()
            .filter(|title| {
                if !bare_precolon(title, &ctx.titles) {
                    return true;
                }
                !precolon_names_a_sibling(title, &ctx.siblings)
            })
            .collect();
        let mut wanted = Vec::new();
        let mut keep_numbers = HashSet::new();
        for title in &titles {
            let words = norm(title);
            if words.is_empty() {
                continue;
            }
            // The word form stays a title option. The digit form is a second
            // option when it differs, and it is where kept numbers come from.
            let digits_form = fold_number_words(words.clone());
            for token in &digits_form {
                if digits(token) {
                    keep_numbers.insert(token.clone());
                }
            }
            if digits_form != words {
                wanted.push(digits_form);
            }
            wanted.push(words);
        }
        // Same work: the main title, the subtitle line, or an alternative.
        // The pre-colon fragment is not one of those, so it cannot drop a sibling.
        let identity: HashSet<Vec<String>> = titles
            .iter()
            .filter(|title| !bare_precolon(title, &ctx.titles))
            .map(|title| norm_words(title))
            .filter(|tokens| !tokens.is_empty())
            .collect();
        let siblings = ctx
            .siblings
            .iter()
            .filter_map(|title| {
                let tokens = norm_words(title);
                if tokens.is_empty() || identity.contains(&tokens) {
                    None
                } else {
                    Some((title.clone(), tokens))
                }
            })
            .collect::<Vec<_>>();
        let mut authors = Vec::new();
        for author in &ctx.authors {
            for token in norm_words(author) {
                if !authors.iter().any(|have: &String| have == &token) {
                    authors.push(token);
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
        let tokens = self.norm_release(text);
        let author_seen = tokens
            .iter()
            .any(|token| self.authors.iter().any(|author| eq(token, author)));
        let release = tokens
            .into_iter()
            .filter(|token| {
                if digits(token) && !self.keep_numbers.contains(token) {
                    return false;
                }
                let author = self.authors.iter().any(|author| eq(token, author));
                if author && self.in_wanted(token) {
                    return true;
                }
                !author
            })
            .collect();
        (release, author_seen)
    }

    /// An author token that is part of the title stays. "Malcolm X" is the book.
    fn in_wanted(&self, token: &str) -> bool {
        self.wanted
            .iter()
            .any(|option| option.iter().any(|have| eq(token, have)))
    }

    fn norm_release(&self, text: &str) -> Vec<String> {
        let folded: String = text.chars().filter_map(fold).collect();
        let mut out = Vec::new();
        for token in folded.split_whitespace() {
            if token.is_empty() || STOP.contains(&token) || ordinal(token) {
                continue;
            }
            // A tag word that is part of a wanted title is the title, not a tag.
            if TAGS.contains(&token) && !self.has_token(token) {
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
}

pub(crate) fn verify_opf(
    matcher: &Matcher,
    ctx: &WorkContext,
    meta: &OpfMeta,
    languages: &[String],
) -> Opf {
    if let Some(language) = meta
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        // Language before ISBN: a matching ISBN in an unaccepted language is
        // still a mismatch. `und` / `mul` / `zxx` are no language.
        if !super::languages::accepts(languages, language) {
            return Opf::Mismatch(format!("Downloaded book is in {language}"));
        }
    }
    let confirmed = meta
        .identifiers
        .iter()
        .any(|raw| isbn13s(raw).iter().any(|isbn| ctx.isbns.contains(isbn)));
    if confirmed {
        return Opf::Confirmed;
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

/// Title text: stopwords, ordinals, and roman numerals. Number words stay
/// words. Edition tags stay, so "The Collector" and "The Illustrated Man"
/// survive. [`Matcher`] also keeps the digit form when it differs.
pub(crate) fn norm(text: &str) -> Vec<String> {
    norm_words(text)
}

/// Stopwords, ordinals, and roman numerals. Number words stay words. Sibling
/// identity uses this so "Mistborn" does not have to share a digit form.
fn norm_words(text: &str) -> Vec<String> {
    let folded: String = text.chars().filter_map(fold).collect();
    let mut out = Vec::new();
    for token in folded.split_whitespace() {
        if token.is_empty() || STOP.contains(&token) || ordinal(token) {
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

/// Drop siblings that normalize to the main title, `title: subtitle`, or an
/// alternative title. A pre-colon fragment is not in `identity`.
pub(crate) fn without_this_work(siblings: Vec<String>, identity: &[String]) -> Vec<String> {
    let forms: HashSet<Vec<String>> = identity
        .iter()
        .map(|title| norm_words(title))
        .filter(|tokens| !tokens.is_empty())
        .collect();
    siblings
        .into_iter()
        .filter(|title| {
            let tokens = norm_words(title);
            !tokens.is_empty() && !forms.contains(&tokens)
        })
        .collect()
}

/// The pre-colon fragment names one of the author's other works.
pub(crate) fn precolon_names_a_sibling(head: &str, siblings: &[String]) -> bool {
    let tokens = norm_words(head);
    !tokens.is_empty() && siblings.iter().any(|title| norm_words(title) == tokens)
}

fn bare_precolon(title: &str, titles: &[String]) -> bool {
    let title = title.trim();
    titles.iter().any(|other| {
        let Some((head, tail)) = other.split_once(':') else {
            return false;
        };
        head.trim() == title && !tail.trim().is_empty() && other.trim() != title
    })
}

fn fold_number_words(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((value, next)) = parse_number(&tokens, i) {
            let text = value.to_string();
            if out.iter().any(|have: &String| have == &text) {
                i = next;
                continue;
            }
            out.push(text);
            i = next;
        } else {
            let token = tokens[i].clone();
            if out.iter().all(|have: &String| have != &token) {
                out.push(token);
            }
            i += 1;
        }
    }
    out
}

fn small_number(word: &str) -> Option<u32> {
    Some(match word {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn parse_small(tokens: &[String], i: usize) -> Option<(u32, usize)> {
    let value = small_number(tokens.get(i)?)?;
    if (20..100).contains(&value) && value.is_multiple_of(10) {
        if let Some(ones) = tokens.get(i + 1).and_then(|word| small_number(word)) {
            if ones < 10 {
                return Some((value + ones, i + 2));
            }
        }
        return Some((value, i + 1));
    }
    if value < 20 {
        return Some((value, i + 1));
    }
    None
}

/// "nineteen eighty-four" is 1984. A lone "four" is 4. Hundreds and thousands
/// are the usual English grouping.
fn parse_number(tokens: &[String], i: usize) -> Option<(u32, usize)> {
    if tokens.get(i).map(String::as_str) == Some("hundred") {
        return Some((100, i + 1));
    }
    if tokens.get(i).map(String::as_str) == Some("thousand") {
        return Some((1000, i + 1));
    }
    let (first, mut j) = parse_small(tokens, i)?;
    if tokens.get(j).map(String::as_str) == Some("hundred") {
        let mut value = first.saturating_mul(100);
        j += 1;
        if let Some((rest, next)) = parse_small(tokens, j) {
            if rest < 100 {
                value = value.saturating_add(rest);
                j = next;
            }
        }
        return Some((value, j));
    }
    if tokens.get(j).map(String::as_str) == Some("thousand") {
        let mut value = first.saturating_mul(1000);
        j += 1;
        if let Some((rest, next)) = parse_number(tokens, j) {
            if rest < 1000 {
                value = value.saturating_add(rest);
                j = next;
            }
        }
        return Some((value, j));
    }
    if (10..100).contains(&first) {
        if let Some((second, next)) = parse_small(tokens, j) {
            if second < 100 {
                return Some((first.saturating_mul(100).saturating_add(second), next));
            }
        }
    }
    Some((first, j))
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

    type Sample = (
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static str,
        Kind,
        f32,
        bool,
    );

    fn failure_cases() -> Vec<Sample> {
        vec![
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
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Frank Herbert - Dune 1",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Dune Book 1",
                Kind::Match,
                1.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Dune - Book I",
                Kind::Match,
                1.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Dune (Dune Chronicles #1)",
                Kind::Match,
                2.0 / 3.0,
                false,
            ),
            (
                "Dune",
                &["Frank Herbert"][..],
                "herbert",
                "Dune (Movie Tie-In)",
                Kind::Match,
                1.0,
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
                "Nineteen Eighty-Four",
                &["George Orwell"][..],
                "orwell",
                "George Orwell - 1984 (retail) (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Nineteen Eighty-Four",
                &["George Orwell"][..],
                "orwell",
                "George Orwell - Nineteen Eighty-Four (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Two Towers",
                &["J.R.R. Tolkien"][..],
                "",
                "J.R.R. Tolkien - The Two Towers (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Two Towers",
                &["J.R.R. Tolkien"][..],
                "",
                "J.R.R. Tolkien - The 2 Towers (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "One Hundred Years of Solitude",
                &["Gabriel Garcia Marquez"][..],
                "",
                "Gabriel Garcia Marquez - One Hundred Years of Solitude (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "One Hundred Years of Solitude",
                &["Gabriel Garcia Marquez"][..],
                "",
                "Gabriel Garcia Marquez - 100 Years of Solitude (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Three-Body Problem",
                &["Liu Cixin"][..],
                "",
                "Liu Cixin - The Three-Body Problem (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Three-Body Problem",
                &["Liu Cixin"][..],
                "",
                "Liu Cixin - Three Body Problem (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "The Three-Body Problem",
                &["Liu Cixin"][..],
                "",
                "Liu Cixin - The 3-Body Problem (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Three Body Problem",
                &["Liu Cixin"][..],
                "",
                "Liu Cixin - Three Body Problem (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Three Body Problem",
                &["Liu Cixin"][..],
                "",
                "Liu Cixin - 3 Body Problem (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Slaughterhouse-Five",
                &["Kurt Vonnegut"][..],
                "",
                "Kurt Vonnegut - Slaughterhouse-Five (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Slaughterhouse-Five",
                &["Kurt Vonnegut"][..],
                "",
                "Kurt Vonnegut - Slaughterhouse 5 (epub)",
                Kind::Match,
                1.0,
                true,
            ),
            (
                "Nineteen Eighty-Four",
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
        ]
    }

    #[test]
    fn failure_corpus_matches_the_author_s_other_works() {
        let herbert = titles("herbert.json");
        let sanderson = titles("sanderson.json");
        let orwell = titles("orwell.json");
        for (title, authors, fixture, release, kind, fit, author_seen) in failure_cases() {
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
        let herbert = titles("herbert.json");
        let sanderson = titles("sanderson.json");
        let orwell = titles("orwell.json");
        let cases = failure_cases();
        for (title, authors, fixture, release, _, _, _) in cases {
            let siblings = match fixture {
                "herbert" => herbert.clone(),
                "sanderson" => sanderson.clone(),
                "orwell" => orwell.clone(),
                "hemingway" => vec!["The Sun Also Rises".into()],
                _ => Vec::new(),
            };
            let matcher = Matcher::new(&ctx(title, authors, siblings));
            let original = matcher.judge(release);
            for tag in TAGS.iter().copied().chain(["40th"]) {
                let tagged = format!("{release} {tag}");
                assert_eq!(matcher.judge(&tagged), original, "{tagged}");
            }
            for author in authors {
                for token in norm_words(author) {
                    let with_author = format!("{release} {token}");
                    let updated = matcher.judge(&with_author);
                    match (&original, &updated) {
                        (
                            Verdict::Match { fit, author_seen },
                            Verdict::Match {
                                fit: again,
                                author_seen: seen,
                            },
                        ) => {
                            assert!(near(*fit, *again), "{with_author}");
                            if *author_seen {
                                assert!(*seen, "{with_author}");
                                assert_eq!(updated, original, "{with_author}");
                            }
                        }
                        _ => assert_eq!(updated, original, "{with_author}"),
                    }
                }
            }
        }
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

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().copied().map(str::to_string).collect()
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
        let languages = codes(&["en"]);
        assert_eq!(
            verify_opf(&matcher, &context, &book.meta, &languages),
            Opf::Mismatch("Downloaded book is in de".into())
        );
        let english = parsed(
            "<dc:title>Chapterhouse Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:identifier>urn:isbn:978-0-441-17271-9</dc:identifier><dc:language>en</dc:language>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &english.meta, &languages),
            Opf::Confirmed
        );
    }

    #[test]
    fn opf_sibling_title_language_and_junk_title() {
        let context = dune_meta(None);
        let matcher = Matcher::new(&context);
        let languages = codes(&["en"]);
        let sibling =
            parsed("<dc:title>Chapterhouse Dune</dc:title><dc:creator>Frank Herbert</dc:creator>");
        assert_eq!(
            verify_opf(&matcher, &context, &sibling.meta, &languages),
            Opf::Mismatch("Downloaded book is Chapterhouse Dune".into())
        );
        let junk =
            parsed("<dc:title>Microsoft Word - doc1</dc:title><dc:creator>User</dc:creator>");
        assert_eq!(junk.meta.titles, vec!["Microsoft Word - doc1".to_string()]);
        assert_eq!(
            verify_opf(&matcher, &context, &junk.meta, &languages),
            Opf::Unknown
        );
        let german = parsed(
            "<dc:title>Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:language>de</dc:language>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &german.meta, &languages),
            Opf::Mismatch("Downloaded book is in de".into())
        );
        let english = parsed(
            "<dc:title>Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:language>en-US</dc:language>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &english.meta, &languages),
            Opf::Confirmed
        );
        for language in ["und", "mul", "zxx"] {
            let missing = parsed(&format!(
                "<dc:title>Dune</dc:title><dc:creator>Frank Herbert</dc:creator><dc:language>{language}</dc:language>"
            ));
            assert_eq!(
                verify_opf(&matcher, &context, &missing.meta, &languages),
                Opf::Confirmed,
                "{language}"
            );
        }
    }

    #[test]
    fn direct_opf_meta_without_a_file_follows_the_same_order() {
        let context = dune_meta(Some("0441172717"));
        let matcher = Matcher::new(&context);
        let german = OpfMeta {
            titles: vec!["Something else".into()],
            creators: vec![],
            identifiers: vec!["0-441-17271-7".into()],
            language: Some("de".into()),
        };
        let languages = codes(&["en"]);
        assert_eq!(
            verify_opf(&matcher, &context, &german, &languages),
            Opf::Mismatch("Downloaded book is in de".into())
        );
        let und = OpfMeta {
            titles: vec!["Something else".into()],
            creators: vec![],
            identifiers: vec!["0-441-17271-7".into()],
            language: Some("und".into()),
        };
        assert_eq!(
            verify_opf(&matcher, &context, &und, &languages),
            Opf::Confirmed
        );
    }

    #[test]
    fn accepted_languages_decide_the_opf_check() {
        let context = dune_meta(Some("9780441172719"));
        let matcher = Matcher::new(&context);
        let meta = |language: &str| OpfMeta {
            titles: vec!["Chapterhouse Dune".into()],
            creators: vec!["Frank Herbert".into()],
            identifiers: vec!["9780441172719".into()],
            language: Some(language.into()),
        };
        let both = codes(&["en", "de"]);
        assert_eq!(
            verify_opf(&matcher, &context, &meta("de"), &both),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("ger"), &both),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("de-DE"), &both),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("fr"), &both),
            Opf::Mismatch("Downloaded book is in fr".into())
        );
        let english = codes(&["en"]);
        assert_eq!(
            verify_opf(&matcher, &context, &meta("de"), &english),
            Opf::Mismatch("Downloaded book is in de".into())
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("en-US"), &english),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("en_GB"), &english),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("eng"), &english),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("English"), &english),
            Opf::Confirmed
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("english"), &english),
            Opf::Confirmed
        );
        let german = codes(&["de"]);
        assert_eq!(
            verify_opf(&matcher, &context, &meta("English"), &german),
            Opf::Mismatch("Downloaded book is in English".into())
        );
        assert_eq!(
            verify_opf(&matcher, &context, &meta("German"), &german),
            Opf::Confirmed
        );
    }

    #[test]
    fn a_precolon_fragment_does_not_swallow_the_sibling() {
        let context = WorkContext {
            titles: vec!["Mistborn: Secret History".into(), "Mistborn".into()],
            authors: vec!["Brandon Sanderson".into()],
            isbns: HashSet::new(),
            siblings: vec!["Mistborn".into(), "The Final Empire".into()],
        };
        let verdict = Matcher::new(&context).judge("Brandon Sanderson - Mistborn (retail) (epub)");
        assert_eq!(verdict, Verdict::NotTitle);
    }

    #[test]
    fn tag_words_in_a_title_are_kept() {
        assert_eq!(norm("The Collector"), vec!["collector".to_string()]);
        assert_eq!(
            norm("The Illustrated Man"),
            vec!["illustrated".to_string(), "man".to_string()]
        );
        let collector = Matcher::new(&ctx("The Collector", &["John Fowles"], vec![]));
        assert!(matches!(
            collector.judge("John Fowles - The Collector (epub)"),
            Verdict::Match { fit, author_seen: true } if near(fit, 1.0)
        ));
        let illustrated = Matcher::new(&ctx(
            "The Illustrated Man",
            &["Ray Bradbury"],
            vec!["The Martian Chronicles".into()],
        ));
        assert!(matches!(
            illustrated.judge("Ray Bradbury - The Illustrated Man (retail) (epub)"),
            Verdict::Match { fit, author_seen: true } if near(fit, 1.0)
        ));
    }

    #[test]
    fn an_author_token_inside_the_title_is_not_stripped() {
        let malcolm = Matcher::new(&ctx(
            "The Autobiography of Malcolm X",
            &["Malcolm X"],
            vec![],
        ));
        assert!(matches!(
            malcolm.judge("Malcolm X - The Autobiography of Malcolm X (epub)"),
            Verdict::Match { fit, author_seen: true } if near(fit, 1.0)
        ));
        let agatha = Matcher::new(&ctx(
            "Agatha Christie: An Autobiography",
            &["Agatha Christie"],
            vec![],
        ));
        assert!(matches!(
            agatha.judge("Agatha Christie - An Autobiography (epub)"),
            Verdict::Match { fit, author_seen: true } if near(fit, 1.0)
        ));
    }

    #[test]
    fn nineteen_eighty_four_folds_to_digits() {
        assert_eq!(
            norm("Nineteen Eighty-Four"),
            vec![
                "nineteen".to_string(),
                "eighty".to_string(),
                "four".to_string()
            ]
        );
        let matcher = Matcher::new(&ctx(
            "Nineteen Eighty-Four",
            &["George Orwell"],
            titles("orwell.json"),
        ));
        for release in [
            "George Orwell - Nineteen Eighty-Four (epub)",
            "George Orwell - 1984 (retail) (epub)",
        ] {
            assert!(
                matches!(
                    matcher.judge(release),
                    Verdict::Match { fit, author_seen: true } if near(fit, 1.0)
                ),
                "{release}"
            );
        }
    }

    #[test]
    fn a_chronicles_epub_is_still_dune() {
        let context = ctx("Dune", &["Frank Herbert"], titles("herbert.json"));
        let matcher = Matcher::new(&context);
        let book = parsed(
            "<dc:title>Dune (Dune Chronicles, Book 1)</dc:title><dc:creator>Frank Herbert</dc:creator>",
        );
        assert_eq!(
            verify_opf(&matcher, &context, &book.meta, &codes(&["en"])),
            Opf::Confirmed
        );
    }
}
