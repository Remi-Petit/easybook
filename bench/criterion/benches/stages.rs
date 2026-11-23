//! Où partent les millisecondes d'une planche : chaque étape de
//! [`easybook::resize::shrink_page`] mesurée seule (1 200 x 1 884 -> 800 px).
//!
//! Sert à **choisir quoi optimiser**, pas à garder une référence : c'est
//! `benches/resize.rs` qui donne le chiffre de production. Les étapes sont
//! réimplémentées ici avec les mêmes crates et les mêmes versions que
//! `backend/` (voir `Cargo.toml`) — si la chaîne de `resize.rs` change, il faut
//! remettre ce fichier en phase.
//!
//! Décodeur et encodeur alternatifs mesurés au passage : `zune-jpeg` à la place
//! de `image`/`jpeg-decoder`, et les filtres de `fast_image_resize`.

mod common;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbImage;
use std::hint::black_box;
use std::io::Cursor;

/// Largeur cible d'une planche (l'une des `resize.widths` de `config.yml`).
const TARGET: u32 = 800;

/// Hauteur qui garde le ratio de l'original.
fn height_for(width: u32, height: u32, target: u32) -> u32 {
    u32::try_from(u64::from(height) * u64::from(target) / u64::from(width.max(1)))
        .unwrap_or(1)
        .max(1)
}

/// Réduction SIMD, comme `resize::scale` (privée) de `backend/`.
fn scale(source: &RgbImage, width: u32, height: u32, alg: ResizeAlg) -> RgbImage {
    let (source_width, source_height) = source.dimensions();
    let image = Image::from_vec_u8(
        source_width,
        source_height,
        source.as_raw().clone(),
        PixelType::U8x3,
    )
    .expect("image source");
    let mut target = Image::new(width, height, PixelType::U8x3);
    Resizer::new()
        .resize(&image, &mut target, &ResizeOptions::new().resize_alg(alg))
        .expect("réduction");
    RgbImage::from_raw(width, height, target.into_vec()).expect("image réduite")
}

fn stages(c: &mut Criterion) {
    let (width, height) = (1200u32, 1884u32);
    let (target_width, target_height) = (TARGET, height_for(width, height, TARGET));
    let page = common::jpeg(width, height, 85);
    println!(
        "planche {width}x{height} ({} Ko) -> {target_width}x{target_height}",
        page.len() / 1024
    );

    let mut group = c.benchmark_group("resize/stages");
    group.sample_size(20);

    // 0. En-tête : `width_of` + `image_size` (sans décoder).
    group.bench_function("header", |b| {
        b.iter(|| {
            black_box(
                image::ImageReader::new(Cursor::new(black_box(&page)))
                    .with_guessed_format()
                    .expect("format")
                    .into_dimensions()
                    .expect("dimensions"),
            )
        });
    });

    // 1. Décodage : celui de `shrink_page` (`image::load_from_memory` quand le
    //    décodage à l'échelle n'est pas possible) contre `zune-jpeg`.
    group.bench_function("decode_image", |b| {
        b.iter(|| black_box(image::load_from_memory(black_box(&page)).expect("décodage image")));
    });
    group.bench_function("decode_zune", |b| {
        b.iter(|| {
            let mut decoder = zune_jpeg::JpegDecoder::new(Cursor::new(black_box(&page)));
            black_box(decoder.decode().expect("décodage zune-jpeg"))
        });
    });

    // 2. Réduction : le filtre décide du coût. Catmull-Rom est celui du code ;
    //    `Hamming` est annoncé par `fast_image_resize` au prix de `Bilinear`
    //    pour une qualité proche du bicubique — le candidat à mesurer.
    let rgb = image::load_from_memory(&page).expect("décodage").into_rgb8();
    for (name, alg) in [
        ("Lanczos3", ResizeAlg::Convolution(FilterType::Lanczos3)),
        ("CatmullRom", ResizeAlg::Convolution(FilterType::CatmullRom)),
        ("Hamming", ResizeAlg::Convolution(FilterType::Hamming)),
        ("Bilinear", ResizeAlg::Convolution(FilterType::Bilinear)),
        ("Nearest", ResizeAlg::Nearest),
    ] {
        group.bench_with_input(BenchmarkId::new("scale", name), &alg, |b, alg| {
            b.iter(|| {
                black_box(scale(
                    black_box(&rgb),
                    target_width,
                    target_height,
                    *alg,
                ))
            });
        });
    }

    // 3. Encodage JPEG de la planche réduite, à la qualité de `config.yml` (80).
    let small = scale(
        &rgb,
        target_width,
        target_height,
        ResizeAlg::Convolution(FilterType::CatmullRom),
    );
    // Taille des pixels en mémoire (largeur x hauteur x 3), ce que l'encodeur
    // reçoit — pas la taille du JPEG produit.
    println!(
        "planche réduite : {target_width}x{target_height} = {} Mpx",
        small.as_raw().len() / 1_048_576
    );
    for quality in [70u8, 80, 90] {
        group.bench_with_input(BenchmarkId::new("encode", quality), &quality, |b, quality| {
            b.iter(|| {
                let mut out = Vec::new();
                jpeg_encoder::Encoder::new(&mut out, *quality)
                    .encode(
                        small.as_raw(),
                        u16::try_from(target_width).expect("largeur"),
                        u16::try_from(target_height).expect("hauteur"),
                        jpeg_encoder::ColorType::Rgb,
                    )
                    .expect("encodage");
                black_box(out)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, stages);
criterion_main!(benches);
