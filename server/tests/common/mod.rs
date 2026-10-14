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
        zip.start_file(*path, SimpleFileOptions::default().compression_method(method))
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub fn epub() -> Vec<u8> {
    book(true, 0)
}

const CHAPTER: &str = "<html><body><h1>I</h1><p>First&nbsp; paragraph &amp; second.</p><p>Last paragraph.</p></body></html>";

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
