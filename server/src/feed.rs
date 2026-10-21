use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    /// `id` of the block this text belongs to, when it has one.
    pub id: String,
}

struct TocEntry {
    path: String,
    fragment: String,
    title: String,
    /// Other labels for this same target that name a different chapter.
    others: Vec<String>,
}

/// Feed assembly from Android's FeedExtractor.kt / FeedStart.kt.
///
/// TOC entries arrive in depth-first order. Two entries are one target when
/// they share an href with no fragment, or the same fragment on that href.
/// They collapse only when the labels name the same chapter; the more
/// descriptive one wins. A different label keeps the first, which is what a
/// part and its opening chapter need when both point at the file. A different
/// fragment stays a nested section. Basename matching still ignores the
/// directory, as on the phones. An in-file heading replaces a contents label
/// only when it names that same chapter, so "1. Introductory" and
/// "INTRODUCTORY" are one row and a translator's note is not.
pub(crate) fn assemble(elements: Vec<TextElement>, toc: Vec<(String, String)>) -> Vec<Post> {
    let entries = collapse_toc(toc);
    let mut file_order = Vec::new();
    let mut file_title: HashMap<String, String> = HashMap::new();
    let mut file_others: HashMap<String, Vec<String>> = HashMap::new();
    let mut sections: HashMap<(String, String), String> = HashMap::new();
    let mut seen_path = HashSet::new();
    for entry in &entries {
        if seen_path.insert(entry.path.clone()) {
            file_order.push(entry.path.clone());
        }
    }
    for path in &file_order {
        let indexes: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.path == *path)
            .map(|(index, _)| index)
            .collect();
        let chosen = indexes
            .iter()
            .copied()
            .find(|index| entries[*index].fragment.is_empty())
            .unwrap_or(indexes[0]);
        file_title.insert(path.clone(), entries[chosen].title.clone());
        file_others.insert(path.clone(), entries[chosen].others.clone());
        for index in indexes {
            if index == chosen {
                continue;
            }
            let entry = &entries[index];
            sections.insert(
                (entry.path.clone(), entry.fragment.clone()),
                entry.title.clone(),
            );
        }
    }

    // Chrome detection uses the nav label. A heading inside the file must not
    // pull a copyright page back into the book.
    let start = file_order
        .iter()
        .find(|path| !is_publisher_chrome(&file_title[*path]))
        .cloned();

    let mut first_heading_id: HashMap<String, String> = HashMap::new();
    let mut first_heading_text: HashMap<String, String> = HashMap::new();
    for element in &elements {
        let path = href_key(&element.href);
        if !element.heading || first_heading_text.contains_key(&path) {
            continue;
        }
        let text = element.text.trim();
        if text.is_empty() {
            continue;
        }
        first_heading_text.insert(path.clone(), text.to_owned());
        if !element.id.is_empty() {
            first_heading_id.insert(path, element.id.clone());
        }
    }
    let mut folded = Vec::new();
    for (path, id) in &first_heading_id {
        let Some(section) = sections.get(&(path.clone(), id.clone())) else {
            continue;
        };
        let heading = first_heading_text
            .get(path)
            .map(String::as_str)
            .unwrap_or("");
        let better = {
            let Some(title) = file_title.get(path) else {
                continue;
            };
            // Fold only when this fragment names the file's chapter. A different
            // label, even on the first heading's id, stays its own section.
            if !same_heading(section, title) {
                continue;
            }
            let mut better = prefer(title, section).to_owned();
            if same_heading(heading, &better) {
                better = prefer(&better, heading).to_owned();
            }
            better
        };
        file_title.insert(path.clone(), better);
        folded.push((path.clone(), id.clone()));
    }
    for key in folded {
        sections.remove(&key);
    }
    for (path, heading) in &first_heading_text {
        let Some(title) = file_title.get(path) else {
            continue;
        };
        // "Chapter One" must not become the translator's note that opens the file.
        if same_heading(heading, title) {
            let better = prefer(title, heading).to_owned();
            file_title.insert(path.clone(), better);
        }
    }

    let mut started = start.is_none();
    let mut seen_files = HashSet::new();
    let mut emitted_sections = HashSet::new();
    let mut chapter = "Start".to_owned();
    let mut posts = Vec::new();
    for element in elements {
        let text = element.text.trim();
        if text.is_empty() {
            continue;
        }
        let key = href_key(&element.href);
        if !started {
            if start.as_ref().is_some_and(|href| href == &key) {
                started = true;
            } else {
                continue;
            }
        }
        let toc_start = file_title.contains_key(&key) && seen_files.insert(key.clone());
        if toc_start {
            chapter.clone_from(&file_title[&key]);
            add(&mut posts, PostKind::Heading, &chapter, &chapter);
        }
        if !element.id.is_empty() {
            let target = (key.clone(), element.id.clone());
            if let Some(title) = sections.get(&target) {
                if emitted_sections.insert(target) {
                    // A fragment on a paragraph is still that section, but the
                    // paragraph is body text. Only a heading element can name it.
                    chapter = if element.heading {
                        prefer(title, text).to_owned()
                    } else {
                        title.clone()
                    };
                    add(&mut posts, PostKind::Heading, &chapter, &chapter);
                    if element.heading || same_heading(text, &chapter) {
                        continue;
                    }
                }
            }
        }
        if toc_start {
            // The opening block is the contents heading when it names that
            // chapter. A heading that names another label for this same target
            // is that chapter. Anything else, including a translator's note,
            // is body.
            if !same_heading(text, &file_title[&key]) {
                let sibling = element.heading.then(|| {
                    file_others
                        .get(&key)
                        .and_then(|labels| labels.iter().find(|label| same_heading(label, text)))
                });
                if let Some(Some(label)) = sibling {
                    chapter = prefer(label, text).to_owned();
                    add(&mut posts, PostKind::Heading, &chapter, &chapter);
                } else {
                    add(&mut posts, PostKind::Body, text, &chapter);
                }
            }
            continue;
        }
        if element.heading {
            if !same_heading(text, &chapter) {
                chapter = text.to_owned();
                add(&mut posts, PostKind::Heading, text, &chapter);
            }
            continue;
        }
        add(&mut posts, PostKind::Body, text, &chapter);
    }
    posts
}

/// One label per target. `href` may still carry the fragment `resolve` kept.
fn collapse_toc(toc: Vec<(String, String)>) -> Vec<TocEntry> {
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut entries: Vec<TocEntry> = Vec::new();
    for (href, title) in toc {
        let title = title.trim();
        if title.is_empty() {
            continue;
        }
        let Some((path, fragment)) = toc_target(&href) else {
            continue;
        };
        let key = (path, fragment);
        if let Some(slot) = index.get(&key) {
            // "Part I" and "Chapter 1: The Storm" share the file and are not
            // one chapter. "1. Introductory" and "INTRODUCTORY" are.
            if same_heading(&entries[*slot].title, title) {
                entries[*slot].title = prefer(&entries[*slot].title, title).to_owned();
            } else if !entries[*slot]
                .others
                .iter()
                .any(|other| same_heading(other, title))
            {
                entries[*slot].others.push(title.to_owned());
            }
        } else {
            index.insert(key.clone(), entries.len());
            entries.push(TocEntry {
                path: key.0,
                fragment: key.1,
                title: title.to_owned(),
                others: Vec::new(),
            });
        }
    }
    entries
}

/// Path basename, plus the fragment when the link has one.
fn toc_target(href: &str) -> Option<(String, String)> {
    let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
    let path = href_key(path);
    if path.is_empty() {
        None
    } else {
        Some((path, fragment.trim().to_owned()))
    }
}

/// Letters carry the title, a chapter number breaks a tie with the all-caps
/// heading, and title case beats a shout of the same words.
fn description_rank(title: &str) -> (usize, usize, usize, usize) {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    let digits = title.chars().filter(|c| c.is_ascii_digit()).count();
    let lower = title.chars().filter(|c| c.is_lowercase()).count();
    let len = title.chars().count();
    (letters, digits, lower, len)
}

fn prefer<'a>(kept: &'a str, candidate: &'a str) -> &'a str {
    if description_rank(candidate) > description_rank(kept) {
        candidate
    } else {
        kept
    }
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
    if body.is_empty() || title.is_empty() {
        return false;
    }
    if body == title
        || body == title.strip_prefix("chapter").unwrap_or(&title)
        || body == title.strip_prefix("part").unwrap_or(&title)
    {
        return true;
    }
    // "Part I" and "Chapter I" both reduce to "i" once the section word is
    // removed. They are different chapters, so do not compare them further.
    let body_word = section_word(&body);
    let title_word = section_word(&title);
    if body_word.is_some() && title_word.is_some() && body_word != title_word {
        return false;
    }
    // "1. Introductory" and "INTRODUCTORY" name one chapter.
    let body = strip_section_number(&body);
    let title = strip_section_number(&title);
    !body.is_empty() && body == title
}

fn section_word(value: &str) -> Option<&'static str> {
    if value
        .strip_prefix("chapter")
        .is_some_and(|rest| !rest.is_empty())
    {
        Some("chapter")
    } else if value
        .strip_prefix("part")
        .is_some_and(|rest| !rest.is_empty())
    {
        Some("part")
    } else {
        None
    }
}

fn strip_section_number(value: &str) -> &str {
    let value = value
        .strip_prefix("chapter")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(value);
    let value = value
        .strip_prefix("part")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(value);
    let stripped = value.trim_start_matches(|c: char| c.is_ascii_digit());
    if stripped.is_empty() {
        value
    } else {
        stripped
    }
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
