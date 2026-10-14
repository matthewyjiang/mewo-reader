use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PostKind {
    Heading,
    Body,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Post {
    pub id: String,
    pub kind: PostKind,
    pub text: String,
    pub chapter: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Posts {
    pub posts: Vec<Post>,
}

pub(crate) struct TextElement {
    pub href: String,
    pub text: String,
    pub heading: bool,
}

/// Exact feed assembly from Android's FeedExtractor.kt / FeedStart.kt. TOC entries
/// arrive in depth-first order; keep the first non-empty title for each href key.
/// Intentionally compare basenames, not full paths or fragments, like the phones.
pub(crate) fn assemble(elements: Vec<TextElement>, toc: Vec<(String, String)>) -> Vec<Post> {
    let mut titles = Vec::new();
    let mut keys = HashSet::new();
    for (href, title) in toc {
        let key = href_key(&href);
        let title = title.trim().to_owned();
        if !key.is_empty() && !title.is_empty() && keys.insert(key.clone()) {
            titles.push((key, title));
        }
    }
    let start = titles.iter().find(|(_, title)| !is_publisher_chrome(title));
    let mut started = start.is_none();
    let mut seen = HashSet::new();
    let mut chapter = "Start".to_owned();
    let mut posts = Vec::new();
    for element in elements {
        let text = element.text.trim();
        if text.is_empty() {
            continue;
        }
        let key = href_key(&element.href);
        if !started {
            if start.is_some_and(|(href, _)| *href == key) {
                started = true;
            } else {
                continue;
            }
        }
        let title = titles.iter().find(|(href, _)| *href == key).map(|(_, t)| t);
        let toc_start = title.is_some() && seen.insert(key);
        if let Some(title) = title.filter(|_| toc_start) {
            chapter = title.clone();
            add(&mut posts, PostKind::Heading, title, &chapter);
            if same_heading(text, title) {
                continue;
            }
        } else if element.heading {
            chapter = text.to_owned();
            add(&mut posts, PostKind::Heading, text, &chapter);
            continue;
        }
        add(&mut posts, PostKind::Body, text, &chapter);
    }
    posts
}

fn add(posts: &mut Vec<Post>, kind: PostKind, text: &str, chapter: &str) {
    posts.push(Post {
        id: posts.len().to_string(),
        kind,
        text: text.to_owned(),
        chapter: chapter.to_owned(),
    });
}

pub(crate) fn href_key(raw: &str) -> String {
    raw.split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .trim_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn alphanumeric(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

fn same_heading(text: &str, title: &str) -> bool {
    let body = alphanumeric(text);
    let title = alphanumeric(title);
    !body.is_empty()
        && !title.is_empty()
        && (body == title
            || body == title.strip_prefix("chapter").unwrap_or(&title)
            || body == title.strip_prefix("part").unwrap_or(&title))
}

fn is_publisher_chrome(title: &str) -> bool {
    let mut normalized = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            normalized.push(c);
        } else if !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    let title = normalized.trim();
    const TITLES: &[&str] = &[
        "cover",
        "title page",
        "copyright",
        "copyright page",
        "contents",
        "table of contents",
        "toc",
        "other books by this author",
        "other books",
        "also by this author",
        "also by",
        "books by this author",
        "by the same author",
        "half title",
        "colophon",
        "frontispiece",
        "imprint",
        "about the publisher",
        "publisher s note",
        "publishers note",
        "translator s note",
        "translators note",
        "about this edition",
        "about the ebook",
        "about the e book",
    ];
    TITLES.contains(&title)
        || ["also by ", "other books ", "books by "]
            .iter()
            .any(|prefix| title.starts_with(prefix))
}

/// Android LocalLibraryStore.slug: lowercase ASCII alphanumerics, first 18,
/// with "author" for a name that contains none.
pub(crate) fn slug(value: &str) -> String {
    let slug: String = alphanumeric(value).chars().take(18).collect();
    if slug.is_empty() {
        "author".into()
    } else {
        slug
    }
}
