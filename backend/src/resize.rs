//! Planches à la taille de l'écran : réduction d'une page de bande dessinée.
//!
//! Une planche scannée fait souvent 1 à 2 Mo pour 1900 pixels de large, sur un
//! écran qui en affiche 400 à 1200. Le lecteur demande donc une **largeur**
//! (`?w=`), arrondie ici à l'une des largeurs de `config.yml` (`resize.widths`) :
//! l'URL reste ainsi dans un petit nombre de variantes, que nginx garde.
//!
//! Une planche déjà plus étroite que la largeur voulue n'est jamais retouchée :
//! recompresser n'apporterait que de la perte.

use crate::config::Resize;
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, RgbImage};
use std::io::Cursor;

/// Décode un JPEG en le réduisant dès la transformée en cosinus (1/2, 1/4 ou 1/8).
///
/// L'image qui sort tient **au moins** dans `max_width` x `max_height` : l'appelant
/// finit la réduction à la taille exacte. Une planche de 1900 px décodée pour
/// une vignette de 400 px n'est ainsi jamais matérialisée en 5 millions de pixels.
///
/// `None` pour ce qui n'est pas un JPEG en niveaux de gris ou RVB (CMYK, autre
/// format), et quand l'image n'est pas au moins deux fois plus grande que la
/// boîte : sans réduction possible, le décodeur de `image` est plus rapide que
/// `jpeg-decoder` (mesuré : +25 % de CPU pour une planche de 1200 px vers 800).
/// L'appelant retombe alors sur le décodage complet.
pub fn decode_jpeg_scaled(bytes: &[u8], max_width: u32, max_height: u32) -> Option<DynamicImage> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if width == 0 || height == 0 {
        return None;
    }
    // Taille voulue : l'image réduite pour tenir dans la boîte, ratio gardé.
    let ratio =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    if ratio > 0.5 {
        return None;
    }
    let wanted_width = (f64::from(width) * ratio).ceil().clamp(1.0, 65535.0) as u16;
    let wanted_height = (f64::from(height) * ratio).ceil().clamp(1.0, 65535.0) as u16;
    decoder.scale(wanted_width, wanted_height).ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        jpeg_decoder::PixelFormat::L8 => {
            let rgb = pixels.iter().flat_map(|gray| [*gray; 3]).collect();
            RgbImage::from_raw(width, height, rgb).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}

/// Largeur retenue pour une demande de `wanted` pixels : la plus petite largeur
/// autorisée qui la couvre, ou la plus grande si aucune ne suffit.
pub fn snap(wanted: u32, allowed: &[u32]) -> Option<u32> {
    let mut sorted: Vec<u32> = allowed.iter().copied().filter(|w| *w > 0).collect();
    sorted.sort_unstable();
    sorted
        .iter()
        .copied()
        .find(|width| *width >= wanted)
        .or_else(|| sorted.last().copied())
}

/// Largeur en pixels de l'image, lue dans son en-tête (sans la décoder).
fn width_of(bytes: &[u8]) -> Option<u32> {
    image_size(bytes).map(|(width, _)| width)
}

/// Dimensions de l'image, lues dans son en-tête (sans la décoder).
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Image RGB réduite à `width` x `height` (filtre Catmull-Rom).
///
/// `fast_image_resize` calcule avec des instructions SIMD : la réduction par
/// `image` (sans SIMD) représentait la majorité du CPU d'une planche.
fn scale(rgb: RgbImage, width: u32, height: u32) -> Option<RgbImage> {
    let (source_width, source_height) = rgb.dimensions();
    let source =
        Image::from_vec_u8(source_width, source_height, rgb.into_raw(), PixelType::U8x3).ok()?;
    let mut target = Image::new(width, height, PixelType::U8x3);
    Resizer::new()
        .resize(
            &source,
            &mut target,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom)),
        )
        .ok()?;
    RgbImage::from_raw(width, height, target.into_vec())
}

/// La page réduite à `target` pixels de large, en JPEG.
///
/// `None` : l'image d'origine doit être servie telle quelle (déjà assez étroite,
/// ou illisible par ce serveur).
pub fn shrink_page(bytes: &[u8], target: u32, settings: &Resize) -> Option<Vec<u8>> {
    if width_of(bytes)? <= target {
        return None;
    }
    let (source_width, source_height) = image_size(bytes)?;
    let height = u32::try_from(
        u64::from(source_height) * u64::from(target) / u64::from(source_width.max(1)),
    )
    .ok()?
    .max(1);
    // Un JPEG est décodé directement à l'échelle utile ; le reste, en entier.
    let image = decode_jpeg_scaled(bytes, target, height)
        .or_else(|| image::load_from_memory(bytes).ok())?;
    let rgb = scale(image.into_rgb8(), target, height)?;

    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, settings.quality)
        .encode(
            rgb.as_raw(),
            u16::try_from(rgb.width()).ok()?,
            u16::try_from(rgb.height()).ok()?,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    // Une « réduction » plus lourde que l'original (image déjà très compressée)
    // n'en est pas une.
    (out.len() < bytes.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgb([
                (x * 7 % 256) as u8,
                (y * 5 % 256) as u8,
                ((x + y) % 256) as u8,
            ]);
        }
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
            .unwrap();
        out
    }

    #[test]
    fn a_width_is_rounded_up_to_an_allowed_one() {
        let allowed = [1600, 800, 1200];
        assert_eq!(snap(300, &allowed), Some(800));
        assert_eq!(snap(800, &allowed), Some(800));
        assert_eq!(snap(801, &allowed), Some(1200));
        // Plus grand que tout : la plus grande largeur autorisée.
        assert_eq!(snap(5000, &allowed), Some(1600));
        assert_eq!(snap(100, &[]), None);
    }

    #[test]
    fn a_jpeg_is_decoded_at_a_reduced_scale() {
        let source = jpeg(1600, 2400);

        // Boîte de 400 px : le décodage s'arrête à 1/2 ou 1/4, jamais sous la taille voulue.
        let small = decode_jpeg_scaled(&source, 400, 600).expect("décodé");
        assert!(small.width() < 1600 && small.width() >= 400);
        assert_eq!(small.width() * 3, small.height() * 2, "ratio gardé");

        // Plus grande que l'image, ou réduction inférieure à 2 : l'appelant décode
        // en entier.
        assert!(decode_jpeg_scaled(&source, 4000, 6000).is_none());
        assert!(decode_jpeg_scaled(&source, 1000, 1500).is_none());

        // Ce n'est pas un JPEG : l'appelant décode autrement.
        assert!(decode_jpeg_scaled(b"pas une image", 400, 600).is_none());
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(8, 8))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert!(decode_jpeg_scaled(&png, 4, 4).is_none());
    }

    #[test]
    fn a_wide_page_is_shrunk_and_keeps_its_ratio() {
        let source = jpeg(1600, 2400);
        let small = shrink_page(&source, 800, &Resize::default()).expect("réduite");
        assert!(small.len() < source.len());

        let decoded = image::load_from_memory(&small).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (800, 1200));
    }

    #[test]
    fn a_narrow_page_is_left_alone() {
        let source = jpeg(600, 900);
        assert!(shrink_page(&source, 800, &Resize::default()).is_none());
        assert!(shrink_page(&source, 600, &Resize::default()).is_none());
    }

    #[test]
    fn something_that_is_not_an_image_is_left_alone() {
        assert!(shrink_page(b"pas une image", 800, &Resize::default()).is_none());
    }
}
