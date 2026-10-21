use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        // Deflate keeps the text-bomb fixture tiny; mimetype stays stored like a real EPUB.
        let method = if *path == "mimetype" {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        zip.start_file(
            *path,
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub fn epub() -> Vec<u8> {
    book(true, 0)
}

const CHAPTER: &str = "<html><body><h1>I</h1><p>First&nbsp; paragraph &amp; second.</p><p>Last paragraph.</p></body></html>";

/// Minimal EPUB3 whose package metadata is `metadata` (already-escaped XML).
#[allow(dead_code)] // used by epub/matching unit tests and requests.rs, not api.rs
pub fn with_metadata(metadata: &str) -> Vec<u8> {
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata>{metadata}</metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#
    );
    zip_files(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OPS/package.opf", opf.as_bytes()),
        ("OPS/chapter.xhtml", b"<html><body><p>Hello.</p></body></html>"),
    ])
}

/// EPUB3 fixture whose chapter document is `chapter`.
#[allow(dead_code)] // used by src/epub.rs unit tests, not tests/api.rs
pub fn with_chapter(chapter: &str) -> Vec<u8> {
    build(true, 0, chapter, None)
}

/// NCX-only fixture whose NCX is not XML. Readium treats that as no TOC.
#[allow(dead_code)] // used by src/epub.rs unit tests, not tests/api.rs
pub fn broken_ncx() -> Vec<u8> {
    build(false, 0, CHAPTER, Some("<ncx><navMap>"))
}

/// Chapter nav entries repeated against the same target, plus real subsections.
///
/// `intro.xhtml` lists "1. Introductory" and a nested "INTRODUCTORY" at the
/// same href. `power.xhtml#divine-omnipotence` repeats the chapter heading.
/// `nest.xhtml` keeps `#pain` and `#heaven` as different sections, and collapses
/// the two labels that share `#pain`.
#[allow(dead_code)] // used by src/epub.rs unit tests, not tests/api.rs
pub fn repeated_chapters(epub3: bool) -> Vec<u8> {
    let (toc_item, toc_path, toc) = if epub3 {
        (
            r#"<item id="toc" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            "OPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="intro.xhtml">1. Introductory</a><ol><li><a href="intro.xhtml">INTRODUCTORY</a></li></ol></li><li><a href="power.xhtml">2. Divine Omnipotence</a><ol><li><a href="power.xhtml#divine-omnipotence">DIVINE OMNIPOTENCE</a></li></ol></li><li><a href="nest.xhtml">3. The Fall</a><ol><li><a href="nest.xhtml#pain">Human Pain</a></li><li><a href="nest.xhtml#pain">HUMAN PAIN</a></li><li><a href="nest.xhtml#heaven">Heaven</a></li></ol></li></ol></nav></body></html>"#,
        )
    } else {
        (
            r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
            "OPS/toc.ncx",
            r#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="intro"><navLabel><text>1. Introductory</text></navLabel><content src="intro.xhtml"/><navPoint id="intro-dup"><navLabel><text>INTRODUCTORY</text></navLabel><content src="intro.xhtml"/></navPoint></navPoint><navPoint id="power"><navLabel><text>2. Divine Omnipotence</text></navLabel><content src="power.xhtml"/><navPoint id="power-dup"><navLabel><text>DIVINE OMNIPOTENCE</text></navLabel><content src="power.xhtml#divine-omnipotence"/></navPoint></navPoint><navPoint id="fall"><navLabel><text>3. The Fall</text></navLabel><content src="nest.xhtml"/><navPoint id="pain"><navLabel><text>Human Pain</text></navLabel><content src="nest.xhtml#pain"/></navPoint><navPoint id="pain-dup"><navLabel><text>HUMAN PAIN</text></navLabel><content src="nest.xhtml#pain"/></navPoint><navPoint id="heaven"><navLabel><text>Heaven</text></navLabel><content src="nest.xhtml#heaven"/></navPoint></navPoint></navMap></ncx>"#,
        )
    };
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="{}"><metadata><dc:title>The Problem of Pain</dc:title><dc:creator>C. S. Lewis</dc:creator></metadata><manifest>{toc_item}<item id="intro" href="intro.xhtml" media-type="application/xhtml+xml"/><item id="power" href="power.xhtml" media-type="application/xhtml+xml"/><item id="nest" href="nest.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="toc"><itemref idref="intro"/><itemref idref="power"/><itemref idref="nest"/></spine></package>"#,
        if epub3 { "3.0" } else { "2.0" }
    );
    zip_files(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OPS/package.opf", opf.as_bytes()),
        (toc_path, toc.as_bytes()),
        ("OPS/intro.xhtml", b"<html><body><p>1</p><h1>INTRODUCTORY</h1><p>The problem of pain.</p></body></html>"),
        ("OPS/power.xhtml", b"<html><body><h1 id=\"divine-omnipotence\">DIVINE OMNIPOTENCE</h1><p>Omnipotence means power.</p></body></html>"),
        ("OPS/nest.xhtml", b"<html><body><h1>The Fall</h1><p>Opening.</p><h2 id=\"pain\">HUMAN PAIN</h2><p>Pain text.</p><h2 id=\"heaven\">HEAVEN</h2><p>Heaven text.</p><h2>A Distinct Argument</h2><p>Argument text.</p></body></html>"),
    ])
}

/// A part and its first chapter share `p1.xhtml` with no fragment. The in-file
/// headings are the part and the chapter, with an epigraph between them.
#[allow(dead_code)] // used by src/epub.rs unit tests, not tests/api.rs
pub fn part_and_chapter(epub3: bool) -> Vec<u8> {
    let (toc_item, toc_path, toc) = if epub3 {
        (
            r#"<item id="toc" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            "OPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="p1.xhtml">Part One</a><ol><li><a href="p1.xhtml">Chapter 1</a></li></ol></li></ol></nav></body></html>"#,
        )
    } else {
        (
            r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
            "OPS/toc.ncx",
            r#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="part"><navLabel><text>Part One</text></navLabel><content src="p1.xhtml"/><navPoint id="ch"><navLabel><text>Chapter 1</text></navLabel><content src="p1.xhtml"/></navPoint></navPoint></navMap></ncx>"#,
        )
    };
    labeled_book(
        epub3,
        toc_item,
        toc_path,
        toc,
        &[(
            "p1",
            "p1.xhtml",
            "<html><body><h1>Part One</h1><p>Epigraph.</p><h2>Chapter 1</h2></body></html>",
        )],
    )
}

/// Labels that share a target but do not name one chapter, a first heading
/// that is not the contents entry, and a fragment that points at a paragraph.
#[allow(dead_code)] // used by src/epub.rs unit tests, not tests/api.rs
pub fn separate_labels(epub3: bool) -> Vec<u8> {
    let (toc_item, toc_path, toc) = if epub3 {
        (
            r#"<item id="toc" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            "OPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="storm.xhtml">Part I</a><ol><li><a href="storm.xhtml">Chapter 1: The Storm</a></li></ol></li><li><a href="book.xhtml#start">Book One</a><ol><li><a href="book.xhtml#start">Chapter 1</a></li></ol></li><li><a href="note.xhtml">Chapter One</a></li><li><a href="para.xhtml">Chapter Two</a><ol><li><a href="para.xhtml#point">The Argument</a></li></ol></li></ol></nav></body></html>"#,
        )
    } else {
        (
            r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
            "OPS/toc.ncx",
            r#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="part"><navLabel><text>Part I</text></navLabel><content src="storm.xhtml"/><navPoint id="storm"><navLabel><text>Chapter 1: The Storm</text></navLabel><content src="storm.xhtml"/></navPoint></navPoint><navPoint id="book"><navLabel><text>Book One</text></navLabel><content src="book.xhtml#start"/><navPoint id="book-ch"><navLabel><text>Chapter 1</text></navLabel><content src="book.xhtml#start"/></navPoint></navPoint><navPoint id="note"><navLabel><text>Chapter One</text></navLabel><content src="note.xhtml"/></navPoint><navPoint id="two"><navLabel><text>Chapter Two</text></navLabel><content src="para.xhtml"/><navPoint id="point"><navLabel><text>The Argument</text></navLabel><content src="para.xhtml#point"/></navPoint></navPoint></navMap></ncx>"#,
        )
    };
    labeled_book(
        epub3,
        toc_item,
        toc_path,
        toc,
        &[
            (
                "storm",
                "storm.xhtml",
                "<html><body><h1>PART I</h1><p>Before the storm.</p><h2>Chapter 1: The Storm</h2><p>Rain.</p></body></html>",
            ),
            (
                "book",
                "book.xhtml",
                "<html><body><h1 id=\"start\">BOOK ONE</h1><p>Verse.</p><h2>Chapter 1</h2><p>After the book.</p></body></html>",
            ),
            (
                "note",
                "note.xhtml",
                "<html><body><h1>A Note on the Translation Used Throughout</h1><p>The translator prefers.</p></body></html>",
            ),
            (
                "para",
                "para.xhtml",
                "<html><body><h1>Chapter Two</h1><p>Opening two.</p><p id=\"point\">This paragraph is long enough to outrank the short toc label and must stay body text.</p></body></html>",
            ),
        ],
    )
}

fn labeled_book(
    epub3: bool,
    toc_item: &str,
    toc_path: &str,
    toc: &str,
    chapters: &[(&str, &str, &str)],
) -> Vec<u8> {
    let manifest: String = chapters
        .iter()
        .map(|(id, href, _)| {
            format!(r#"<item id="{id}" href="{href}" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let spine: String = chapters
        .iter()
        .map(|(id, _, _)| format!(r#"<itemref idref="{id}"/>"#))
        .collect();
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="{}"><metadata><dc:title>Labels</dc:title><dc:creator>Ada</dc:creator></metadata><manifest>{toc_item}{manifest}</manifest><spine toc="toc">{spine}</spine></package>"#,
        if epub3 { "3.0" } else { "2.0" }
    );
    let mut entries = vec![
        (
            "mimetype".to_owned(),
            b"application/epub+zip".to_vec(),
        ),
        (
            "META-INF/container.xml".to_owned(),
            br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
        ),
        ("OPS/package.opf".to_owned(), opf.into_bytes()),
        (toc_path.to_owned(), toc.as_bytes().to_vec()),
    ];
    entries.extend(
        chapters
            .iter()
            .map(|(_, href, body)| (format!("OPS/{href}"), body.as_bytes().to_vec())),
    );
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    zip_files(&borrowed)
}

/// Both EPUB3 nav and EPUB2 NCX fixtures include publisher chrome, a duplicate
/// nested TOC href, an HTML entity, metadata fallbacks and a PNG cover.
pub fn book(epub3: bool, padding: usize) -> Vec<u8> {
    build(epub3, padding, CHAPTER, None)
}

fn build(epub3: bool, padding: usize, chapter: &str, toc_override: Option<&str>) -> Vec<u8> {
    let (toc_item, toc_path, toc) = if epub3 {
        (
            r#"<item id="toc" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            "OPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="copyright.xhtml">Publisher’s Note</a></li><li><a href="chapter.xhtml#one">Chapter I</a><ol><li><a href="chapter.xhtml#two">Ignored duplicate title</a></li></ol></li></ol></nav></body></html>"#,
        )
    } else {
        (
            r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
            "OPS/toc.ncx",
            r#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="chrome"><navLabel><text>Publisher’s Note</text></navLabel><content src="copyright.xhtml"/></navPoint><navPoint id="one"><navLabel><text>Chapter I</text></navLabel><content src="chapter.xhtml#one"/><navPoint id="two"><navLabel><text>Ignored duplicate title</text></navLabel><content src="chapter.xhtml#two"/></navPoint></navPoint></navMap></ncx>"#,
        )
    };
    let cover_attributes = if epub3 {
        "properties=\"cover-image\""
    } else {
        ""
    };
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="{}"><metadata><dc:title>Fixture Book</dc:title><dc:creator> </dc:creator><dc:creator>Jane A. Writer</dc:creator><meta name="cover" content="cover"/></metadata><manifest>{toc_item}<item id="chrome" href="copyright.xhtml" media-type="application/xhtml+xml"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="cover.png" media-type="image/png" {cover_attributes}/></manifest><spine toc="toc"><itemref idref="chrome"/><itemref idref="chapter"/></spine></package>"#,
        if epub3 { "3.0" } else { "2.0" }
    );
    let toc = toc_override.unwrap_or(toc);
    let padding = vec![0; padding];
    zip_files(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OPS/package.opf", opf.as_bytes()),
        (toc_path, toc.as_bytes()),
        ("OPS/copyright.xhtml", b"<html><body><h1>Publisher's Note</h1><p>Chrome to skip.</p></body></html>"),
        ("OPS/chapter.xhtml", chapter.as_bytes()),
        ("OPS/cover.png", b"\x89PNG\r\n\x1a\nfixture"),
        ("padding.bin", &padding),
    ])
}
