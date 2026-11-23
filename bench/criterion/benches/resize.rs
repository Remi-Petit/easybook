//! Réduction des planches : décodage JPEG, redimensionnement SIMD, ré-encodage.
//!
//! C'est le poste le plus lourd du serveur (tout le reste est de la copie de
//! quelques kilo-octets). Deux chemins sont mesurés séparément :
//!
//! - 1200 -> 800 px : le ratio (0,67) est trop faible pour que `jpeg-decoder`
//!   décode à l'échelle 1/2, la planche est donc décodée en entier puis réduite ;
//! - 1900 -> 800 px : ratio 0,42, `decode_jpeg_scaled` décode directement à
//!   l'échelle 1/4 — c'est le gain qui a motivé cette fonction.

mod common;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use easybook::config::Resize;
use easybook::resize::{shrink_page, snap};
use std::hint::black_box;

fn shrink(c: &mut Criterion) {
    let settings = Resize::default();
    let mut group = c.benchmark_group("resize/shrink_page");
    // Une planche prend des dizaines de millisecondes : 20 échantillons suffisent
    // à un intervalle de confiance utile, et gardent le banc sous une minute.
    group.sample_size(20);

    for (width, height) in [(1200u32, 1884u32), (1900, 2980)] {
        let page = common::jpeg(width, height, 85);
        group.throughput(Throughput::Bytes(page.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("page", format!("{width}x{height}")),
            &page,
            |b, page| {
                b.iter(|| black_box(shrink_page(black_box(page), 800, &settings)));
            },
        );
    }
    group.finish();
}

fn snap_width(c: &mut Criterion) {
    let widths = Resize::default().widths;
    c.bench_function("resize/snap", |b| {
        b.iter(|| black_box(snap(black_box(1_000), black_box(&widths))));
    });
}

criterion_group!(benches, shrink, snap_width);
criterion_main!(benches);
