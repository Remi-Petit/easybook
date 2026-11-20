//! Fixtures des tests d'intégration : de vrais fichiers, dans un vrai dossier.
//!
//! Les tests passent par le routeur HTTP, pas par les fonctions internes : ce
//! sont donc les formats réels qui sont exercés (archive ZIP, RAR, PDF), et pas
//! des doubles.

#![allow(dead_code)]

pub mod http;
pub mod idp;

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, RgbImage};
use zip::write::SimpleFileOptions;

/// Une image JPEG de test (dégradé, comme une vraie planche).
pub fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut image = RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
    }
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(image)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .unwrap();
    out
}

/// EPUB minimal : deux chapitres, une image de couverture, une feuille de style.
pub fn epub(dir: &Path) -> PathBuf {
    let path = dir.join("livre.epub");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());

    let text_entries: Vec<(&str, String)> = vec![
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?>
            <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
              <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
            </container>"#
                .to_string(),
        ),
        (
            "OEBPS/content.opf",
            r#"<?xml version="1.0"?>
            <package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
              <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
                <dc:title>Livre de test</dc:title>
                <dc:creator>Autrice</dc:creator>
              </metadata>
              <manifest>
                <item id="cover" href="Images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
                <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
                <item id="c1" href="Text/chap1.xhtml" media-type="application/xhtml+xml"/>
                <item id="c2" href="Text/chap2.xhtml" media-type="application/xhtml+xml"/>
                <item id="css" href="Styles/style.css" media-type="text/css"/>
              </manifest>
              <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
            </package>"#
                .to_string(),
        ),
        (
            "OEBPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
              <body><nav epub:type="toc"><ol>
                <li><a href="Text/chap1.xhtml">Début</a></li>
                <li><a href="Text/chap2.xhtml">Suite</a></li>
              </ol></nav></body></html>"#
                .to_string(),
        ),
        (
            "OEBPS/Text/chap1.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
                 <h1>Un</h1><img src="../Images/cover.jpg"/></body></html>"#
                .to_string(),
        ),
        (
            "OEBPS/Text/chap2.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Deux</h1></body></html>"#
                .to_string(),
        ),
        (
            "OEBPS/Styles/style.css",
            "body { background: url(../Images/cover.jpg); }".to_string(),
        ),
    ];

    for (name, content) in text_entries {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }

    zip.start_file("OEBPS/Images/cover.jpg", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&jpeg(64, 96)).unwrap();
    zip.finish().unwrap();

    path
}

/// Bande dessinée ZIP à `pages` pages (`page1.jpg`, `page2.jpg`, …).
pub fn cbz(dir: &Path, name: &str, pages: usize) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, cbz_bytes(name, pages)).unwrap();
    path
}

/// Le contenu d'une bande dessinée de test, sans passer par le disque.
///
/// Sert au dépôt par l'API : ce qui est envoyé est exactement ce qu'un
/// navigateur enverrait.
pub fn cbz_bytes(name: &str, pages: usize) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        for page in 1..=pages {
            zip.start_file(format!("page{page}.jpg"), SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&jpeg(40, 60)).unwrap();
        }
        zip.finish().unwrap();
    }
    let bytes = out.into_inner();
    assert!(!bytes.is_empty(), "{name} : archive vide");
    bytes
}

/// PDF d'une page, entièrement composée d'une image JPEG.
pub fn pdf(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let (width, height) = (64u32, 96u32);
    let jpeg = jpeg(width, height);
    fs::write(&path, pdf_with_jpeg(&jpeg, width, height)).unwrap();
    path
}

/// Un PDF minimal : catalogue, page unique, image `DCTDecode`, contenu.
///
/// Écrit à la main pour que `lopdf` n'ait pas à reconstruire le document (les
/// décalages `xref` sont justes).
pub fn pdf_with_jpeg(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets: Vec<usize> = Vec::new();

    offsets.push(out.len());
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(out.len());
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets.push(out.len());
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
          /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
    );

    offsets.push(out.len());
    out.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(jpeg);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    let content = format!("q {width} 0 0 {height} 0 0 cm /Im0 Do Q");
    offsets.push(out.len());
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len()
        )
        .as_bytes(),
    );

    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}
