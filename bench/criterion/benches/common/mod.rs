//! Fixtures des bancs : tout est généré à la volée.
//!
//! Un banc qui lit des fichiers écrits à la main finit par mesurer autre chose
//! que le code — la taille du dépôt, la météo du disque. Générer les entrées
//! rend les chiffres comparables d'un poste à l'autre et reproductibles après
//! un `cargo clean`.
//!
//! Chaque fichier de `benches/` inclut ce module par `mod common;` : les
//! fonctions qu'un banc n'utilise pas ne doivent pas produire d'avertissement.
#![allow(dead_code)]

use std::io::Write;
use std::path::Path;

/// Planche scannée courante : 1200 px de large, une page de bande dessinée.
pub const PAGE_WIDTH: u32 = 1200;
/// Hauteur correspondante (ratio d'une planche 1200 x 1884).
pub const PAGE_HEIGHT: u32 = 1884;

/// Image RVB de `width` x `height` encodée en JPEG de qualité `quality`.
///
/// Le contenu est un dégradé bruité, pas un aplat : une image unie se compresse
/// en quelques centaines d'octets et se décode trop vite pour représenter une
/// planche réelle (le décodeur saute les blocs vides).
pub fn jpeg(width: u32, height: u32, quality: u8) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    // Générateur congruentiel : déterministe, donc la même fixture à chaque run.
    let mut state: u32 = 0x1234_5678;
    for y in 0..height {
        for x in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 24) as u8;
            pixels.extend_from_slice(&[
                (x * 255 / width.max(1)) as u8 ^ (noise >> 3),
                (y * 255 / height.max(1)) as u8,
                (noise >> 1) | 0x40,
            ]);
        }
    }

    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&pixels, width, height, image::ExtendedColorType::Rgb8)
        .expect("encodage JPEG de la fixture");
    out
}

/// Écrit un CBZ de `pages` planches dans `path`, avec `page` répétée.
///
/// Les octets de la planche sont fournis par l'appelant : générer 80 images
/// identiques coûterait plus cher que le banc lui-même.
pub fn write_cbz(path: &Path, page: &[u8], pages: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("création du dossier de la fixture");
    }
    let file = std::fs::File::create(path).expect("création du CBZ");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for index in 0..pages {
        zip.start_file(format!("page-{index:03}.jpg"), options)
            .expect("entrée du CBZ");
        zip.write_all(page).expect("écriture de la planche");
    }
    zip.finish().expect("clôture du CBZ");
}

/// Écrit un CBZ d'une planche de `PAGE_WIDTH` x `PAGE_HEIGHT` dans `path`.
pub fn cbz(path: &Path, pages: usize) {
    write_cbz(path, &jpeg(PAGE_WIDTH, PAGE_HEIGHT, 85), pages);
}

/// Arborescence de `books` livres : `books / Serie NN / TNN.cbz`, 10 livres par
/// dossier, une planche chacun — la forme réelle d'une bande dessinée rangée.
pub fn tree(root: &Path, books: usize) {
    // Une seule planche pour tous les livres : c'est le parcours qui est mesuré.
    let page = jpeg(400, 600, 70);
    for index in 0..books {
        let folder = root.join(format!("Serie {:02}", index / 10));
        let file = folder.join(format!("T{:02}.cbz", index % 10));
        write_cbz(&file, &page, 1);
    }
}

/// Chapitre XHTML de `paragraphs` paragraphes, avec liens relatifs, images et
/// une feuille de style en ligne : les trois formes que `rewrite::html` réécrit.
pub fn chapter(paragraphs: usize) -> String {
    let mut out = String::with_capacity(paragraphs * 320);
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
         <head><title>Chapitre</title>\
         <link rel=\"stylesheet\" href=\"../Styles/style.css\"/></head>\n<body>\n",
    );
    for index in 0..paragraphs {
        out.push_str(&format!(
            "<p id=\"p{index}\"><img src=\"../Images/planche-{:03}.jpg\" alt=\"planche {index}\"/>\
             Texte du paragraphe {index}, avec une note <a href=\"notes.xhtml#n{index}\">note {index}</a>.\
             </p>\n",
            index % 32
        ));
    }
    out.push_str(
        "<style>@font-face{font-family:main;src:url(../Fonts/main.otf)}\
         p{margin:0;background:url(../Images/fond.png)}</style>\n</body></html>\n",
    );
    out
}

/// Feuille de style de `rules` règles, chacune avec une URL relative à réécrire.
pub fn stylesheet(rules: usize) -> String {
    let mut out = String::with_capacity(rules * 120);
    for index in 0..rules {
        out.push_str(&format!(
            ".planche-{index:03}{{background-image:url(../Images/planche-{:03}.jpg);\
             margin:{index}px 0 0 0}}\n",
            index % 32
        ));
    }
    out
}
