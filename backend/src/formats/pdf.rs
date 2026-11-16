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

    fn pages(&self, path: &Path) -> Result<Option<usize>, String> {
        let document = Document::load(path).map_err(|e| format!("{} : {e}", path.display()))?;
        Ok(Some(document.get_pages().len()))
    }

    fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        cover(path)
    }
}

/// L'image de la première page, si elle existe.
///
/// C'est **la première image dessinée** par le flux de contenu, pas la plus
/// grosse : une page de garde peut contenir une grande illustration de fond et
/// une vignette de couverture, ou une planche plein format plus légère que
/// l'image décorative. L'ordre du flux est le seul qui dise ce que le lecteur
/// voit d'abord.
fn cover(path: &Path) -> Option<Vec<u8>> {
    let document = Document::load(path).ok()?;
    let page = *document.get_pages().values().next()?;
    let resources = page_resources(&document, page);

    // 1. Suivre le dessin : `Do` dans l'ordre, en descendant dans les formulaires
    //    (`Form XObject`) qui encapsulent la planche.
    let content = document.get_page_content(page);
    for name in drawn_names(&content) {
        if let Some(bytes) = image_named(&document, &resources, &name, 0) {
            return Some(bytes);
        }
    }

    // 2. Repli : la plus grande image de la page (flux illisible ou inhabituel).
    let mut best: Option<(u64, Vec<u8>)> = None;
    for resources in &resources {
        collect_images(&document, resources, &mut best, 0);
    }
    best.map(|(_, bytes)| bytes)
}

/// Dictionnaires de ressources d'une page : le direct, puis les hérités.
fn page_resources(document: &Document, page: lopdf::ObjectId) -> Vec<&Dictionary> {
    let Ok((direct, inherited)) = document.get_page_resources(page) else {
        return Vec::new();
    };

    let mut resources: Vec<&Dictionary> = Vec::new();
    if let Some(direct) = direct {
        resources.push(direct);
    }
    for id in inherited {
        if let Ok(dict) = document.get_dictionary(id) {
            resources.push(dict);
        }
    }
    resources
}

/// Noms des `XObject` dessinés par un flux de contenu, **dans l'ordre**.
///
/// Un appel se lit `/<nom> Do` : on retient le jeton qui précède `Do`. Les
/// opérateurs sont dans le flux décompressé, que `get_page_content` rend déjà
/// lisible.
fn drawn_names(content: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(content);
    let mut names = Vec::new();
    let mut previous: Option<&str> = None;

    for token in text.split_whitespace() {
        if token == "Do"
            && let Some(name) = previous
            && let Some(name) = name.strip_prefix('/')
            && !name.is_empty()
        {
            names.push(name.to_string());
        }
        previous = Some(token);
    }

    names
}

/// Image portant ce nom dans les ressources, en descendant dans les formulaires.
fn image_named(
    document: &Document,
    resources: &[&Dictionary],
    name: &str,
    depth: usize,
) -> Option<Vec<u8>> {
    if depth > 3 {
        return None;
    }

    for resources in resources {
        let Some(xobjects) = dictionary(document, resources, b"XObject") else {
            continue;
        };
        let Ok(object) = xobjects.get(name.as_bytes()) else {
            continue;
        };
        let Ok(id) = object.as_reference() else {
            continue;
        };
        let Ok(Object::Stream(stream)) = document.get_object(id) else {
            continue;
        };

        if is_image(stream) {
            return image_bytes(stream);
        }

        // `Form XObject` : son propre flux dessine ses images.
        if is_subtype(stream, b"Form")
            && let Some(content) = decode_filters(stream)
            && let Some(nested) = dictionary(document, &stream.dict, b"Resources")
        {
            for nested_name in drawn_names(&content) {
                if let Some(bytes) = image_named(document, &[nested], &nested_name, depth + 1) {
                    return Some(bytes);
                }
            }
        }
    }

    None
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
///
/// Un flux **sans** `/Filter` (contenu d'un `Form XObject`, par exemple) est
/// rendu tel quel.
fn decode_filters(stream: &Stream) -> Option<Vec<u8>> {
    let Ok(filters) = stream.filters() else {
        return Some(stream.content.clone());
    };
    let mut data = stream.content.clone();

    for filter in filters {
        match filter {
            b"FlateDecode" => data = inflate(&data)?,
            // Le flux contient déjà une image complète.
            b"DCTDecode" | b"JPXDecode" => return Some(data),
            b"LZWDecode" => data = lzw(&data)?,
            // ASCII85, ASCIIHex, CCITT, JBIG2… : `image` ne saurait pas les lire.
            _ => return None,
        }
    }

    Some(data)
}

/// Plafond de la sortie LZW : un flux minuscule peut se décompresser à l'infini.
const LZW_LIMIT: usize = 256 * 1024 * 1024;

/// Défait un flux LZW (`/EarlyChange` par défaut) avec le décodeur de `lopdf`.
fn lzw(data: &[u8]) -> Option<Vec<u8>> {
    let mut dict = Dictionary::new();
    dict.set("Filter", "LZWDecode");
    Stream::new(dict, data.to_vec())
        .decompressed_content_with_limit(LZW_LIMIT)
        .ok()
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

    /// Une page de garde peut dessiner une petite vignette avant une grande
    /// illustration : c'est la **première** qui est la couverture.
    #[test]
    fn the_first_drawn_image_wins_over_a_larger_one() {
        let first = jpeg(32, 48);
        let later = jpeg(200, 300);

        let content = b"q 32 0 0 48 0 0 cm /First Do Q q 200 0 0 300 0 0 cm /Later Do Q";
        let pdf = assemble(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /First 4 0 R /Later 5 0 R >> >> /Contents 6 0 R >>"
                .to_vec(),
            image_object(&first, 32, 48),
            image_object(&later, 200, 300),
            content_object(content),
        ]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("garde.pdf");
        std::fs::write(&path, pdf).unwrap();

        assert_eq!(PDF.cover(&path).unwrap(), first);
    }

    /// InDesign encapsule la planche dans un `Form XObject` : la suivre dans
    /// l'ordre de dessin donne toujours la bonne image.
    #[test]
    fn follows_drawing_order_inside_form_xobjects() {
        let page_image = jpeg(80, 120);
        let decoy = jpeg(300, 400);

        let pdf = assemble(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Fm 4 0 R /Decoy 6 0 R >> >> /Contents 7 0 R >>"
                .to_vec(),
            // 4 : le formulaire, qui dessine `Page` (5) dans ses ressources.
            form_object(
                b"q 80 0 0 120 0 0 cm /Page Do Q",
                b"<< /XObject << /Page 5 0 R >> >>",
            ),
            image_object(&page_image, 80, 120),
            image_object(&decoy, 300, 400),
            content_object(b"q /Fm Do Q q /Decoy Do Q"),
        ]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("form.pdf");
        std::fs::write(&path, pdf).unwrap();

        assert_eq!(PDF.cover(&path).unwrap(), page_image);
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

    /// PDF d'une page dont l'unique XObject est l'image fournie.
    fn pdf_with_image(image: Vec<u8>) -> Vec<u8> {
        assemble(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"
                .to_vec(),
            image,
            content_object(b"q /Im0 Do Q"),
        ])
    }

    /// Un objet image décrit par son dictionnaire (`/ColorSpace`, `/Filter`…).
    fn described_image(data: &[u8], width: u32, height: u32, extra: &str) -> Vec<u8> {
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} {extra} /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        body
    }

    /// Un scan sans filtre : les pixels sont bruts, il faut les encoder.
    #[test]
    fn raw_rgb_pixels_become_a_cover() {
        let (width, height) = (2u32, 3u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[(x * 40) as u8, (y * 60) as u8, 128]);
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("brut.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                &pixels,
                width,
                height,
                "/ColorSpace /DeviceRGB /BitsPerComponent 8",
            )),
        )
        .unwrap();

        let cover = PDF.cover(&path).expect("pixels encodés en couverture");
        assert_eq!(&cover[..4], b"\x89PNG", "ré-encodée en PNG");
    }

    /// Combinaison la plus courante d'un scan : `FlateDecode` devant des pixels.
    #[test]
    fn a_flate_encoded_image_is_inflated_first() {
        let (width, height) = (4u32, 4u32);
        let pixels: Vec<u8> = (0..(width * height)).map(|i| (i * 16) as u8).collect();

        let mut zlib = Vec::new();
        {
            use std::io::Write;
            let mut encoder =
                flate2::write::ZlibEncoder::new(&mut zlib, flate2::Compression::default());
            encoder.write_all(&pixels).unwrap();
            encoder.finish().unwrap();
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flate.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                &zlib,
                width,
                height,
                "/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
            )),
        )
        .unwrap();

        assert!(PDF.cover(&path).is_some(), "les pixels sont décompressés");
    }

    /// Encode en LZW comme un PDF (MSB, `EarlyChange` par défaut).
    fn lzw_encode(data: &[u8]) -> Vec<u8> {
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .encode(data)
            .unwrap()
    }

    /// Cas réel : des PDF dont la première page est un JPEG passé en LZW
    /// (`/Filter [/LZWDecode /DCTDecode]`).
    #[test]
    fn an_lzw_wrapped_jpeg_becomes_a_cover() {
        let jpeg = jpeg(32, 48);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lzw-jpeg.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                &lzw_encode(&jpeg),
                32,
                48,
                "/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter [/LZWDecode /DCTDecode]",
            )),
        )
        .unwrap();

        assert_eq!(PDF.cover(&path).unwrap(), jpeg);
    }

    #[test]
    fn lzw_encoded_pixels_become_a_cover() {
        let pixels: Vec<u8> = (0..16u8).map(|i| i * 16).collect();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lzw-pixels.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                &lzw_encode(&pixels),
                4,
                4,
                "/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /LZWDecode",
            )),
        )
        .unwrap();

        assert!(PDF.cover(&path).is_some());
    }

    /// Un filtre qu'on ne sait pas défaire (`CCITTFaxDecode`) : pas de fausse
    /// couverture, l'interface affichera son repli.
    #[test]
    fn an_unknown_filter_gives_no_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ccitt.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                b"peu importe",
                4,
                4,
                "/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /CCITTFaxDecode",
            )),
        )
        .unwrap();

        assert!(PDF.cover(&path).is_none());
    }

    /// Les images à 1 bit par composant (fax, bitmap ancien) ne sont pas des
    /// couvertures utilisables ici — et surtout, elles ne plantent pas.
    #[test]
    fn a_one_bit_image_gives_no_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bitmap.pdf");
        std::fs::write(
            &path,
            pdf_with_image(described_image(
                &[0xF0, 0x0F],
                4,
                4,
                "/ColorSpace /DeviceGray /BitsPerComponent 1",
            )),
        )
        .unwrap();

        assert!(PDF.cover(&path).is_none());
    }

    /// Assemble des objets PDF (1..n) avec une table `xref` juste.
    fn assemble(objects: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<usize> = Vec::new();

        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }

        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn image_object(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        body.extend_from_slice(jpeg);
        body.extend_from_slice(b"\nendstream");
        body
    }

    fn content_object(content: &[u8]) -> Vec<u8> {
        let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\nendstream");
        body
    }

    fn form_object(content: &[u8], resources: &[u8]) -> Vec<u8> {
        let mut body = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources {} /Length {} >>\nstream\n",
            String::from_utf8_lossy(resources),
            content.len()
        )
        .into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\nendstream");
        body
    }
}
