//! PDF : le document est affiché par le lecteur du navigateur ; ici, on en
//! extrait seulement **la couverture**.
//!
//! Rendre une page demanderait un moteur complet (MuPDF, pdfium…). Ce n'est pas
//! nécessaire : dans un scan ou un manga, une page est une image, et c'est cette
//! image qu'on prend pour la vignette. Un PDF de texte n'en a pas — l'interface
//! affiche alors son repli.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use lopdf::{Dictionary, Document, Object, Stream};

use crate::formats::BookFormat;

/// Format PDF, tel que l'enregistre [`super::module`].
pub struct Pdf;

/// Instance unique du format.
pub static PDF: Pdf = Pdf;

impl BookFormat for Pdf {
    fn id(&self) -> &'static str {
        "pdf"
    }

    fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        cover(path)
    }
}

/// La plus grande image de la première page, si elle existe.
fn cover(path: &Path) -> Option<Vec<u8>> {
    let document = Document::load(path).ok()?;
    let page = *document.get_pages().values().next()?;

    // Les ressources d'une page sont soit un dictionnaire direct, soit des
    // références héritées du nœud parent : `get_page_resources` rend les deux.
    let (resources, inherited) = document.get_page_resources(page).ok()?;

    let mut best: Option<(u64, Vec<u8>)> = None;
    if let Some(resources) = resources {
        collect_images(&document, resources, &mut best, 0);
    }
    for id in inherited {
        if let Ok(resources) = document.get_dictionary(id) {
            collect_images(&document, resources, &mut best, 0);
        }
    }

    best.map(|(_, bytes)| bytes)
}

/// Parcourt les `XObject` d'un dictionnaire de ressources.
///
/// Un contenu peut être encapsulé dans un `Form XObject` (InDesign le fait
/// souvent) : ses propres ressources sont alors visitées aussi, une profondeur
/// raisonnable pour ne pas suivre un cycle.
fn collect_images(
    document: &Document,
    resources: &Dictionary,
    best: &mut Option<(u64, Vec<u8>)>,
    depth: usize,
) {
    if depth > 2 {
        return;
    }

    let Some(xobjects) = dictionary(document, resources, b"XObject") else {
        return;
    };

    for (_, object) in xobjects.iter() {
        let Ok(id) = object.as_reference() else {
            continue;
        };
        let Ok(object) = document.get_object(id) else {
            continue;
        };
        let Object::Stream(stream) = object else {
            continue;
        };

        if is_image(stream) {
            let Some((width, height)) = dimensions(stream) else {
                continue;
            };
            let area = u64::from(width) * u64::from(height);
            if area == 0 || best.as_ref().is_some_and(|(best, _)| *best >= area) {
                continue;
            }
            if let Some(bytes) = image_bytes(stream) {
                *best = Some((area, bytes));
            }
        } else if is_subtype(stream, b"Form")
            && let Some(nested) = dictionary(document, &stream.dict, b"Resources")
        {
            collect_images(document, nested, best, depth + 1);
        }
    }
}

/// Un dictionnaire désigné directement ou par référence.
fn dictionary<'a>(
    document: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    match dict.get(key).ok()? {
        Object::Reference(id) => document.get_dictionary(*id).ok(),
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

fn is_image(stream: &Stream) -> bool {
    is_subtype(stream, b"Image")
}

fn is_subtype(stream: &Stream, expected: &[u8]) -> bool {
    stream
        .dict
        .get(b"Subtype")
        .is_ok_and(|object| object.as_name().is_ok_and(|name| name == expected))
}

fn dimensions(stream: &Stream) -> Option<(u32, u32)> {
    let width = stream.dict.get(b"Width").ok()?.as_i64().ok()?;
    let height = stream.dict.get(b"Height").ok()?.as_i64().ok()?;
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

/// Octets d'une image, prêts à être décodés par la vignette.
///
/// Trois cas, dans l'ordre : l'image est **déjà** encodée (le contenu est le
/// JPEG lui-même) ; elle l'est derrière une couche de compression (`FlateDecode`
/// puis `DCTDecode` — c'est la combinaison la plus courante d'un scan) ; ce sont
/// enfin des pixels bruts (gris ou RVB, 8 bits).
fn image_bytes(stream: &Stream) -> Option<Vec<u8>> {
    // 1. Contenu déjà encodé.
    if image::load_from_memory(&stream.content).is_ok() {
        return Some(stream.content.clone());
    }

    // 2. Filtres : on défait ce qui est une compression de flux, et on s'arrête
    //    dès que le résultat est une image (`DCTDecode` = JPEG, `JPXDecode` =
    //    JPEG 2000).
    let decoded = decode_filters(stream)?;
    if image::load_from_memory(&decoded).is_ok() {
        return Some(decoded);
    }

    // 3. Pixels bruts.
    let raw = decoded;
    let (width, height) = dimensions(stream)?;
    let pixels = (width as usize).checked_mul(height as usize)?;

    let image: DynamicImage =
        match color_space(stream)? {
            ColorSpace::Gray if raw.len() >= pixels => DynamicImage::ImageLuma8(
                GrayImage::from_raw(width, height, raw[..pixels].to_vec())?,
            ),
            ColorSpace::Rgb if raw.len() >= pixels * 3 => DynamicImage::ImageRgb8(
                RgbImage::from_raw(width, height, raw[..pixels * 3].to_vec())?,
            ),
            _ => return None,
        };

    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// Applique les filtres d'un flux dans l'ordre, jusqu'à rencontrer un encodage
/// d'image (qu'on ne décode pas : `image` s'en charge) ou un filtre inconnu.
fn decode_filters(stream: &Stream) -> Option<Vec<u8>> {
    let filters = stream.filters().ok()?;
    let mut data = stream.content.clone();

    for filter in filters {
        match filter {
            b"FlateDecode" => data = inflate(&data)?,
            // Le flux contient déjà une image complète.
            b"DCTDecode" | b"JPXDecode" => return Some(data),
            // ASCII85/ASCIIHex/LZW : rare pour une image ; `image` ne saurait
            // pas les lire de toute façon.
            _ => return None,
        }
    }

    Some(data)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

enum ColorSpace {
    Gray,
    Rgb,
}

fn color_space(stream: &Stream) -> Option<ColorSpace> {
    // `/BitsPerComponent` absent vaut 8 (défaut du format).
    let bits = stream
        .dict
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|object| object.as_i64().ok())
        .unwrap_or(8);
    if bits != 8 {
        return None;
    }

    let value = stream.dict.get(b"ColorSpace").ok()?;
    let name: Vec<u8> = match value {
        Object::Name(name) => name.clone(),
        // `/ICCBased` et compagnie s'écrivent en tableau : la famille est en
        // tête (`[/ICCBased 5 0 R]`).
        Object::Array(items) => items.first()?.as_name().ok()?.to_vec(),
        _ => return None,
    };

    match name.as_slice() {
        b"DeviceGray" | b"CalGray" => Some(ColorSpace::Gray),
        b"DeviceRGB" | b"CalRGB" => Some(ColorSpace::Rgb),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Une image JPEG de test (dégradé, comme une vraie planche).
    pub(crate) fn jpeg(width: u32, height: u32) -> Vec<u8> {
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

    /// Un PDF minimal d'une page, dont la page est une image JPEG.
    ///
    /// Écrit à la main : les crochets `xref` sont recalculés, sinon `lopdf`
    /// reconstruit le document et le test ne vérifierait plus le même fichier.
    pub(crate) fn pdf_with_jpeg(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
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

    #[test]
    fn extracts_the_embedded_image_as_the_cover() {
        let jpeg = jpeg(64, 96);
        let pdf = pdf_with_jpeg(&jpeg, 64, 96);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.pdf");
        std::fs::write(&path, &pdf).unwrap();

        let cover = PDF.cover(&path).expect("couverture extraite");
        assert_eq!(cover, jpeg, "le JPEG embarqué est rendu tel quel");
    }

    #[test]
    fn a_text_only_pdf_has_no_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("texte.pdf");
        // Page sans image : aucun XObject.
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n\
                    trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n";
        std::fs::write(&path, pdf).unwrap();
        assert!(PDF.cover(&path).is_none());
    }
}
