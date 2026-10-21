//! EPUB package parsing and Readium-compatible text segmentation. Nothing is
//! extracted to disk: ZIP paths are resolved inside the archive only.
use crate::feed::{assemble, Post, TextElement};
use ego_tree::iter::Edge;
use roxmltree::{Document, Node, ParsingOptions};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::io::{Read, Seek};
use zip::ZipArchive;

pub struct ParsedEpub {
    pub title: Option<String>,
    pub author: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub posts: Vec<Post>,
    pub(crate) meta: OpfMeta,
}

/// Package metadata the request worker checks after a download. Titles and
/// creators keep every `dc:` value, not just the first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpfMeta {
    pub titles: Vec<String>,
    pub creators: Vec<String>,
    pub identifiers: Vec<String>,
    pub language: Option<String>,
}

struct Item {
    path: String,
    media_type: String,
    properties: String,
}

/// Total inflated bytes of XML/XHTML the parser will read from one EPUB.
/// Receipts: the most text measured is Gutenberg War and Peace (epub3.images)
/// at 4_240_089 bytes; its one-file HTML edition is 3_878_334; a retail novel
/// is 1_070_316. 32 MiB is about 8x the biggest and stops a ZIP bomb before
/// html5ever builds a DOM for it. Covers have their own budget.
pub const MAX_EPUB_TEXT_BYTES: u64 = 32 * 1024 * 1024;

/// Call from spawn_blocking: ZIP inflation, XML and HTML parsing are synchronous.
pub fn parse_file(path: &std::path::Path, max_cover_bytes: u64) -> Result<ParsedEpub, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    parse(file, max_cover_bytes)
}

pub fn parse<R: Read + Seek>(reader: R, max_cover_bytes: u64) -> Result<ParsedEpub, String> {
    let zip = ZipArchive::new(reader).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
    let mut zip = Budgeted {
        zip,
        left: MAX_EPUB_TEXT_BYTES,
    };
    let container = zip.read_text("META-INF/container.xml")?;
    let container = xml(&container)?;
    let opf_path = container
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or("Missing package path in container.xml")?;
    let opf_path = resolve("", opf_path)?;
    let opf_text = zip.read_text(&opf_path)?;
    let opf = xml(&opf_text)?;
    let metadata = opf
        .descendants()
        .find(|n| n.has_tag_name("metadata"))
        .ok_or("Missing package metadata")?;
    let dc_values = |name| {
        metadata
            .descendants()
            .filter(|n| n.has_tag_name(("http://purl.org/dc/elements/1.1/", name)))
            .map(node_text)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    };
    let titles = dc_values("title");
    let creators = dc_values("creator");
    let identifiers = dc_values("identifier");
    let language = dc_values("language").into_iter().next();
    let title = titles.first().cloned();
    let author = creators.first().cloned();
    let manifest_node = opf
        .descendants()
        .find(|n| n.has_tag_name("manifest"))
        .ok_or("Missing package manifest")?;
    let mut manifest = HashMap::new();
    // Preserve manifest order when choosing nav and cover candidates.
    let mut order = Vec::new();
    for item in manifest_node.children().filter(|n| n.has_tag_name("item")) {
        let id = item
            .attribute("id")
            .ok_or("Manifest item is missing an id")?;
        let href = item
            .attribute("href")
            .ok_or("Manifest item is missing an href")?;
        order.push(id.to_owned());
        manifest.insert(
            id.to_owned(),
            Item {
                path: resolve(&opf_path, href)?,
                media_type: item.attribute("media-type").unwrap_or("").to_owned(),
                properties: item.attribute("properties").unwrap_or("").to_owned(),
            },
        );
    }
    let spine = opf
        .descendants()
        .find(|n| n.has_tag_name("spine"))
        .ok_or("Missing package spine")?;
    let mut toc = Vec::new();
    if let Some(nav) = order
        .iter()
        .filter_map(|id| manifest.get(id))
        .find(|item| has_token(&item.properties, "nav"))
    {
        // An invalid/missing nav falls back to NCX, as does a nav without a TOC.
        // The text budget is the one read error that must not be swallowed.
        match zip.read_text(&nav.path) {
            Ok(text) => {
                if let Ok(entries) = nav_toc(&text, &nav.path) {
                    toc = entries;
                }
            }
            Err(err) if zip.left == 0 => return Err(err),
            Err(_) => {}
        }
    }
    if toc.is_empty() {
        let ncx = spine
            .attribute("toc")
            .and_then(|id| manifest.get(id))
            .or_else(|| {
                order
                    .iter()
                    .filter_map(|id| manifest.get(id))
                    .find(|item| item.media_type == "application/x-dtbncx+xml")
            });
        // Like Readium's parseNcx, a missing or broken NCX means no TOC, not a bad book.
        if let Some(ncx) = ncx {
            match zip.read_text(&ncx.path) {
                Ok(text) => toc = ncx_toc(&text, &ncx.path).unwrap_or_default(),
                Err(err) if zip.left == 0 => return Err(err),
                Err(_) => {}
            }
        }
    }
    let cover_id = metadata
        .descendants()
        .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))
        .and_then(|n| n.attribute("content"));
    let cover_item = order
        .iter()
        .filter_map(|id| manifest.get(id))
        .find(|item| has_token(&item.properties, "cover-image"))
        .or_else(|| cover_id.and_then(|id| manifest.get(id)));
    let cover = if let Some(item) = cover_item {
        // Cover is optional; never inflate a cover above the existing upload budget.
        match zip.zip.by_name(&item.path) {
            Ok(mut file) if file.size() <= max_cover_bytes => {
                let mut bytes = Vec::new();
                (&mut file)
                    .take(max_cover_bytes.saturating_add(1))
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() as u64 <= max_cover_bytes {
                    Some(bytes)
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };
    let mut elements = Vec::new();
    let mut spine_items = 0;
    for reference in spine.children().filter(|n| n.has_tag_name("itemref")) {
        // Readium puts linear="no" resources in resources, not readingOrder.
        if reference.attribute("linear") == Some("no") {
            continue;
        }
        let id = reference
            .attribute("idref")
            .ok_or("Spine item is missing an idref")?;
        let item = manifest
            .get(id)
            .ok_or_else(|| format!("Unknown spine item: {id}"))?;
        spine_items += 1;
        if matches!(
            item.media_type.as_str(),
            "application/xhtml+xml" | "text/html"
        ) {
            let html = zip.read_text(&item.path)?;
            elements.extend(text_elements(&html, &item.path));
        }
    }
    if spine_items == 0 {
        return Err("The EPUB has no reading-order documents".into());
    }
    let posts = assemble(elements, toc);
    if posts.is_empty() {
        return Err("The EPUB has no readable text after its front matter".into());
    }
    Ok(ParsedEpub {
        title,
        author,
        cover,
        posts,
        meta: OpfMeta {
            titles,
            creators,
            identifiers,
            language,
        },
    })
}

fn has_token(value: &str, token: &str) -> bool {
    value.split_whitespace().any(|t| t == token)
}

/// ZIP archive that charges every inflated text read against MAX_EPUB_TEXT_BYTES,
/// including repeated spine references.
struct Budgeted<R> {
    zip: ZipArchive<R>,
    left: u64,
}

impl<R: Read + Seek> Budgeted<R> {
    fn read_text(&mut self, path: &str) -> Result<String, String> {
        let over = |asked: u64| {
            format!(
                "text inflates past the MAX_EPUB_TEXT_BYTES budget of {MAX_EPUB_TEXT_BYTES} bytes \
                 (asked at least {asked})"
            )
        };
        let used = MAX_EPUB_TEXT_BYTES - self.left;
        let file = self
            .zip
            .by_name(path)
            .map_err(|e| format!("Cannot open {path}: {e}"))?;
        // The declared size can lie, so it only rejects early; take() enforces.
        if file.size() > self.left {
            self.left = 0;
            return Err(over(used + file.size()));
        }
        let mut bytes = Vec::new();
        file.take(self.left + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read {path}: {e}"))?;
        if bytes.len() as u64 > self.left {
            self.left = 0;
            return Err(over(used + bytes.len() as u64));
        }
        self.left -= bytes.len() as u64;
        let text = String::from_utf8(bytes).map_err(|e| format!("Cannot read {path}: {e}"))?;
        Ok(text.trim_start_matches('\u{feff}').to_owned())
    }
}

fn xml(text: &str) -> Result<Document<'_>, String> {
    // EPUB2 NCX often declares an external DTD. roxmltree does not fetch it.
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("Invalid EPUB XML: {e}"))
}

fn node_text(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>()
        .trim()
        .to_owned()
}

fn nav_toc(text: &str, path: &str) -> Result<Vec<(String, String)>, String> {
    let doc = xml(text)?;
    let nav = doc.descendants().find(|n| {
        n.has_tag_name("nav")
            && n.attribute(("http://www.idpf.org/2007/ops", "type"))
                .is_some_and(|v| has_token(v, "toc"))
    });
    let Some(nav) = nav else {
        return Ok(Vec::new());
    };
    // XML descendants are pre-order: parent TOC entries precede nested children.
    nav.descendants()
        .filter(|n| n.has_tag_name("a"))
        .filter_map(|n| n.attribute("href").map(|href| (href, node_text(n))))
        .map(|(href, title)| toc_ref(path, href).map(|href| (href, title)))
        .collect()
}

fn ncx_toc(text: &str, path: &str) -> Result<Vec<(String, String)>, String> {
    let doc = xml(text)?;
    let Some(nav) = doc.descendants().find(|n| n.has_tag_name("navMap")) else {
        return Ok(Vec::new());
    };
    nav.descendants()
        .filter(|n| n.has_tag_name("navPoint"))
        .filter_map(|point| {
            let label = point.children().find(|n| n.has_tag_name("navLabel"))?;
            let content = point.children().find(|n| n.has_tag_name("content"))?;
            Some((content.attribute("src")?, node_text(label)))
        })
        .map(|(href, title)| toc_ref(path, href).map(|href| (href, title)))
        .collect()
}

/// Keep the fragment. `resolve` drops it so archive paths cannot escape, but
/// `#pain` and `#heaven` are different sections of one file.
fn toc_ref(base: &str, href: &str) -> Result<String, String> {
    let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
    let path = resolve(base, path)?;
    let fragment = fragment.split('?').next().unwrap_or("").trim();
    if fragment.is_empty() {
        Ok(path)
    } else {
        Ok(format!("{path}#{fragment}"))
    }
}

/// Resolve EPUB URI references without ever touching filesystem paths. Decode
/// percent escapes after removing the query/fragment so escaped # stays literal.
fn resolve(base: &str, href: &str) -> Result<String, String> {
    let href = href
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    if href.contains(':') || href.starts_with("//") {
        return Err(format!("External EPUB resource is unsupported: {href}"));
    }
    let mut decoded = Vec::new();
    let mut bytes = href.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes.next().and_then(|b| (b as char).to_digit(16));
            let lo = bytes.next().and_then(|b| (b as char).to_digit(16));
            let (Some(hi), Some(lo)) = (hi, lo) else {
                return Err(format!("Invalid EPUB href: {href}"));
            };
            decoded.push((hi * 16 + lo) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    if decoded.is_empty() {
        return Ok(base.to_owned());
    }
    let parent = base
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    let combined = if decoded.starts_with('/') {
        decoded
    } else {
        format!("{parent}/{decoded}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("EPUB href escapes archive: {href}"));
                }
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Port of Readium Kotlin 3.3.0 HtmlResourceContentIterator (BSD-3-Clause; see
/// THIRD_PARTY_LICENSES.md): JSoup block entry/exit flushes; br/img/audio/video
/// flushes; inline nodes concatenate; language changes split segments, not posts;
/// whitespace-only text nodes are ignored, HTML ASCII whitespace collapses, NBSP
/// survives, zero-width spaces and soft hyphens disappear. CSS does not affect it.
///
/// Deviations: html5ever repairs malformed HTML instead of JSoup; entities decode
/// once (JSoup + Readium unescapes twice); UTF-8 documents only. script/style/head
/// are skipped (JSoup's script/style DataNodes aren't TextNodes); noscript/template
/// are explicitly skipped instead of exposing fallback text. h1-h6 get Heading
/// roles, whereas Kotlin 3.3.0 currently marks every role Body. Heading detection
/// changes kind/chapter, not the number/order of text elements or feed indexes.
/// We omit locators/media elements and retain only text, as FeedExtractor does.
fn text_elements(source: &str, href: &str) -> Vec<TextElement> {
    let html = Html::parse_document(source);
    let selector = Selector::parse("body").expect("static selector");
    let Some(body) = html.select(&selector).next() else {
        return Vec::new();
    };
    let mut parser = TextParser::default();
    let mut blocks: Vec<OpenBlock> = Vec::new();
    let mut skipped = None;
    for edge in body.traverse() {
        match edge {
            Edge::Open(node) => {
                if skipped.is_some() {
                    continue;
                }
                match node.value() {
                    scraper::Node::Element(element) => {
                        let tag = element.name();
                        if matches!(tag, "script" | "style" | "head" | "noscript" | "template") {
                            let (heading, id) = block_state(&blocks);
                            parser.flush(href, heading, id);
                            skipped = Some(node.id());
                            continue;
                        }
                        if is_block(tag) {
                            let (heading, id) = block_state(&blocks);
                            parser.flush(href, heading, id);
                            blocks.push(OpenBlock {
                                heading: matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6"),
                                id: element.attr("id").unwrap_or("").to_owned(),
                            });
                        }
                        if matches!(tag, "br" | "img" | "audio" | "video") {
                            let (heading, id) = block_state(&blocks);
                            parser.flush(href, heading, id);
                        }
                    }
                    scraper::Node::Text(text) if !text.trim().is_empty() => {
                        let language = node
                            .ancestors()
                            .filter_map(|n| n.value().as_element())
                            .find_map(|e| {
                                e.attr("xml:lang")
                                    .filter(|v| !v.trim().is_empty())
                                    .or_else(|| e.attr("lang").filter(|v| !v.trim().is_empty()))
                            })
                            .map(str::to_owned);
                        if language != parser.language {
                            parser.flush_segment();
                            parser.language = language;
                        }
                        parser.append(text);
                    }
                    _ => {}
                }
            }
            Edge::Close(node) => {
                if let Some(id) = skipped {
                    if node.id() == id {
                        skipped = None;
                    }
                    continue;
                }
                if node
                    .value()
                    .as_element()
                    .is_some_and(|e| is_block(e.name()))
                {
                    let (heading, id) = block_state(&blocks);
                    parser.flush(href, heading, id);
                    blocks.pop();
                }
            }
        }
    }
    parser.flush(href, false, "");
    parser.elements
}

struct OpenBlock {
    heading: bool,
    id: String,
}

fn block_state(blocks: &[OpenBlock]) -> (bool, &str) {
    match blocks.last() {
        Some(block) => (block.heading, block.id.as_str()),
        None => (false, ""),
    }
}

fn is_block(tag: &str) -> bool {
    // JSoup 1.22.2 TagSet blockTags (Readium Kotlin 3.3.0 depends on that
    // version), plus SVG/MathML roots. Not CSS display: ins, del, canvas,
    // audio, and video are inline there; audio/video flush on entry only.
    matches!(
        tag,
        "html"
            | "head"
            | "body"
            | "frameset"
            | "script"
            | "noscript"
            | "style"
            | "meta"
            | "link"
            | "title"
            | "frame"
            | "noframes"
            | "section"
            | "nav"
            | "aside"
            | "hgroup"
            | "header"
            | "footer"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "dialog"
            | "search"
            | "ul"
            | "ol"
            | "pre"
            | "div"
            | "blockquote"
            | "hr"
            | "address"
            | "figure"
            | "figcaption"
            | "form"
            | "fieldset"
            | "dl"
            | "dt"
            | "dd"
            | "li"
            | "table"
            | "caption"
            | "thead"
            | "tfoot"
            | "tbody"
            | "colgroup"
            | "col"
            | "tr"
            | "th"
            | "td"
            | "details"
            | "menu"
            | "plaintext"
            | "template"
            | "article"
            | "main"
            | "center"
            | "dir"
            | "applet"
            | "marquee"
            | "listing"
            | "svg"
            | "math"
    )
}

#[derive(Default)]
struct TextParser {
    elements: Vec<TextElement>,
    segments: String,
    text: String,
    language: Option<String>,
}

impl TextParser {
    fn append(&mut self, text: &str) {
        let strip_leading = self.text.ends_with(' ');
        let mut reached_nonwhite = false;
        let mut last_was_white = false;
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r') {
                if (strip_leading && !reached_nonwhite) || last_was_white {
                    continue;
                }
                self.text.push(' ');
                last_was_white = true;
            } else if !matches!(c, '\u{200b}' | '\u{ad}') {
                self.text.push(c);
                last_was_white = false;
                reached_nonwhite = true;
            }
        }
    }

    fn flush_segment(&mut self) {
        if !self.text.trim().is_empty() {
            if self.segments.is_empty() {
                // Readium trims the first segment, retaining one trailing whitespace.
                self.segments.push_str(self.text.trim());
                if let Some(c) = self.text.chars().last().filter(|c| c.is_whitespace()) {
                    self.segments.push(c);
                }
            } else {
                self.segments.push_str(&self.text);
            }
        }
        self.text.clear();
    }

    fn flush(&mut self, href: &str, heading: bool, id: &str) {
        self.flush_segment();
        if !self.segments.trim().is_empty() {
            self.elements.push(TextElement {
                href: href.to_owned(),
                text: self.segments.trim_end().to_owned(),
                heading,
                id: id.to_owned(),
            });
        }
        self.segments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::PostKind;
    use crate::test_epub;
    use std::io::Cursor;

    #[test]
    fn epub3_nav_skips_chrome_and_deduplicates_heading() {
        let book = parse(Cursor::new(test_epub::epub()), 1024).unwrap();
        assert_eq!(book.title.as_deref(), Some("Fixture Book"));
        assert_eq!(book.author.as_deref(), Some("Jane A. Writer"));
        assert_eq!(book.meta.titles, vec!["Fixture Book".to_string()]);
        assert_eq!(book.meta.creators, vec!["Jane A. Writer".to_string()]);
        assert!(book.meta.identifiers.is_empty());
        assert!(book.meta.language.is_none());
        assert_eq!(
            book.cover.as_deref(),
            Some(b"\x89PNG\r\n\x1a\nfixture".as_slice())
        );
        assert_eq!(book.posts.len(), 3);
        assert_eq!(book.posts[0].kind, PostKind::Heading);
        assert_eq!(book.posts[0].text, "Chapter I");
        assert_eq!(book.posts[1].text, "First\u{a0} paragraph & second.");
        assert_eq!(book.posts[1].kind, PostKind::Body);
        assert!(book.posts.iter().all(|p| p.chapter == "Chapter I"));
    }

    #[test]
    fn repeated_chapter_labels_collapse_without_dropping_nested_sections() {
        let headings = [
            "1. Introductory",
            "2. Divine Omnipotence",
            "3. The Fall",
            "Human Pain",
            "Heaven",
            "A Distinct Argument",
        ];
        let bodies = [
            "1",
            "The problem of pain.",
            "Omnipotence means power.",
            "Opening.",
            "Pain text.",
            "Heaven text.",
            "Argument text.",
        ];
        for epub3 in [true, false] {
            let book = parse(Cursor::new(test_epub::repeated_chapters(epub3)), 1024).unwrap();
            let got_headings: Vec<_> = book
                .posts
                .iter()
                .filter(|post| post.kind == PostKind::Heading)
                .map(|post| post.text.as_str())
                .collect();
            assert_eq!(got_headings, headings, "epub3={epub3}");
            let got_bodies: Vec<_> = book
                .posts
                .iter()
                .filter(|post| post.kind == PostKind::Body)
                .map(|post| post.text.as_str())
                .collect();
            assert_eq!(got_bodies, bodies, "epub3={epub3}");
            let pain = book
                .posts
                .iter()
                .find(|post| post.text == "Pain text.")
                .unwrap();
            assert_eq!(pain.chapter, "Human Pain");
            let argument = book.posts.last().unwrap();
            assert_eq!(argument.chapter, "A Distinct Argument");
        }
    }

    #[test]
    fn ncx_only_and_legacy_cover_metadata_match_nav() {
        let ncx = parse(Cursor::new(test_epub::book(false, 0)), 1024).unwrap();
        let nav = parse(Cursor::new(test_epub::epub()), 1024).unwrap();
        assert_eq!(ncx.posts, nav.posts);
        assert_eq!(ncx.cover, nav.cover);
        // Above the existing cover budget is absent, not an EPUB parse failure.
        assert!(parse(Cursor::new(test_epub::epub()), 1)
            .unwrap()
            .cover
            .is_none());
    }

    #[test]
    fn readium_block_boundaries_and_inline_whitespace() {
        let source = "<html><head><title>Not content</title></head><body>\n<div>Outer<p> A\t <em> B </em>\n C &nbsp; D\u{ad}\u{200b}</p>Tail</div><p>one<br>two<img src='x'>three</p><h2>Next</h2><p>left<span> </span><b>right</b></p><script>not content</script><style>not content</style><p lang='en'>Hello <span lang='fr'> monde </span> again</p></body></html>";
        let elements = text_elements(source, "chapter.xhtml");
        let texts: Vec<_> = elements.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Outer",
                "A B C \u{a0} D",
                "Tail",
                "one",
                "two",
                "three",
                "Next",
                "leftright",
                "Hello  monde  again"
            ]
        );
        assert!(elements[6].heading);
        assert!(elements
            .iter()
            .enumerate()
            .all(|(i, e)| e.heading == (i == 6)));
        let posts = assemble(elements, vec![]);
        assert_eq!(posts[0].chapter, "Start");
        assert_eq!(posts[6].kind, PostKind::Heading);
        assert_eq!(posts[7].chapter, "Next");
    }

    #[test]
    fn jsoup_inline_tags_do_not_split_paragraphs() {
        let source = "<body><p>before <ins>added</ins> after</p><p>gone <del>old</del> \
                      now</p><search>found</search><p>a<audio src='x.mp3'></audio>b</p></body>";
        let texts: Vec<_> = text_elements(source, "c.xhtml")
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(
            texts,
            ["before added after", "gone old now", "found", "a", "b"]
        );
    }

    #[test]
    fn text_bomb_is_rejected_with_the_budget() {
        let bomb = " ".repeat(MAX_EPUB_TEXT_BYTES as usize + 1);
        let err = parse(Cursor::new(test_epub::with_chapter(&bomb)), 1024)
            .err()
            .expect("bomb must fail");
        assert!(err.contains("MAX_EPUB_TEXT_BYTES"), "{err}");
        assert!(err.contains(&MAX_EPUB_TEXT_BYTES.to_string()), "{err}");
    }

    #[test]
    fn broken_ncx_is_no_toc_not_a_bad_book() {
        let book = parse(Cursor::new(test_epub::book(false, 0)), 1024).unwrap();
        let broken = parse(Cursor::new(test_epub::broken_ncx()), 1024).unwrap();
        assert!(!broken.posts.is_empty());
        assert_ne!(broken.posts, book.posts);
    }

    #[test]
    fn bundled_sample_has_a_sequential_feed() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../android/app/src/main/assets/sample.epub");
        let book = parse_file(&path, crate::config::MAX_COVER_BYTES).unwrap();
        assert!(!book.posts.is_empty());
        assert_eq!(book.posts[0].kind, PostKind::Heading);
        assert!(book
            .posts
            .iter()
            .enumerate()
            .all(|(i, p)| p.id == i.to_string()));
    }
}
