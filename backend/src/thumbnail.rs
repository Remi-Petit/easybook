//! Couvertures : extraction depuis le livre, vignette, cache mémoire.
//!
//! On ne sert jamais l'image d'origine au catalogue : une couverture d'EPUB pèse
//! souvent plusieurs mégaoctets, une planche de bande dessinée davantage encore,
//! et une page en affiche des dizaines. Chaque couverture est donc réduite une
//! fois, puis gardée en mémoire.
//!
//! D'où vient la couverture, selon le format :
//! - **EPUB** : celle que le livre déclare (voir [`crate::formats::epub`]) ;
//! - **CBZ** et **CBR** : la première page, par convention du format ;
//! - **PDF** : la plus grande image de la première page (un scan, une planche).

use crate::formats::{self, Format};
use image::{DynamicImage, ImageFormat};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Largeur maximale d'une vignette (la hauteur suit le ratio du livre).
const WIDTH: u32 = 400;
/// Nombre de vignettes gardées en mémoire. Au-delà, le cache est vidé : c'est un
/// accélérateur, pas une base de données.
const CAPACITY: usize = 400;

struct Cached {
    bytes: Vec<u8>,
}

/// Couverture d'un livre, en JPEG, à la taille d'une vignette.
///
/// `version` fait partie de la clé du cache : la date de modification du fichier
/// (un livre remplacé change d'image) et, le cas échéant, la révision demandée
/// par l'interface.
pub fn cover(path: &Path, rel: &str, version: &str) -> Option<Vec<u8>> {
    let key = format!("{rel}|{version}");
    if let Some(bytes) = lookup(&key) {
        return Some(bytes);
    }

    let original = match Format::from_path(rel) {
        Some(format) => formats::module(format).cover(path),
        None => None,
    }?;

    // Une image illisible (format exotique) reste mieux qu'une absence : on la
    // sert telle quelle, le navigateur saura peut-être l'afficher.
    let made = shrink(&original).unwrap_or(original);
    store(key, made.clone());
    Some(made)
}

/// Réduit une image à [`WIDTH`] de large, en JPEG.
fn shrink(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumb: DynamicImage = image.thumbnail(WIDTH, WIDTH * 3 / 2);
    let mut out = Vec::new();
    thumb
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup(key: &str) -> Option<Vec<u8>> {
    cache()
        .lock()
        .ok()?
        .get(key)
        .map(|entry| entry.bytes.clone())
}

fn store(key: String, bytes: Vec<u8>) {
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    if cache.len() >= CAPACITY {
        cache.clear();
    }
    cache.insert(key, Cached { bytes });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn thumbnails_are_smaller_and_keep_the_ratio() {
        // Une image « bruitée » : elle ne se comprime pas à presque rien, comme
        // une vraie planche.
        let mut source = RgbaImage::new(900, 1350);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 233) as u8, 255]);
        }
        let mut png = Vec::new();
        source
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        let thumb = shrink(&png).expect("vignette produite");
        assert_eq!(&thumb[..2], &[0xFF, 0xD8], "encodée en JPEG");
        assert!(thumb.len() < png.len(), "la vignette doit être plus légère");

        let decoded = image::load_from_memory(&thumb).unwrap();
        assert_eq!(decoded.width(), WIDTH);
        assert_eq!(decoded.height(), WIDTH * 3 / 2);
    }

    #[test]
    fn unknown_bytes_are_not_a_cover() {
        assert!(shrink(b"pas une image").is_none());
    }
}
