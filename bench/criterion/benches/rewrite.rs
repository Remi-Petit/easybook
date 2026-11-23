//! Réécriture des URL (XHTML et CSS) : le seul poste CPU de la lecture qui ne
//! touche ni au disque ni à une image.
//!
//! Les tailles d'entrée sont paramétrées (50, 500, 5 000 paragraphes) pour
//! vérifier la **croissance** et pas seulement une durée : un chapitre dix fois
//! plus long doit coûter dix fois plus cher. Si l'écart s'envole, la réécriture
//! n'est plus linéaire en la taille du document (régression algorithmique).

mod common;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn rewrite_html(c: &mut Criterion) {
    let mut group = c.benchmark_group("rewrite/html");
    for paragraphs in [50usize, 500, 5_000] {
        let chapter = common::chapter(paragraphs);
        group.bench_with_input(
            BenchmarkId::from_parameter(paragraphs),
            &chapter,
            |b, chapter| {
                b.iter(|| {
                    black_box(easybook::rewrite::html(
                        black_box(chapter),
                        "OEBPS/Text",
                        "Serie 01/T01.epub",
                        "/api/asset",
                    ))
                });
            },
        );
    }
    group.finish();
}

fn rewrite_css(c: &mut Criterion) {
    let mut group = c.benchmark_group("rewrite/css");
    for rules in [50usize, 500, 5_000] {
        let sheet = common::stylesheet(rules);
        group.bench_with_input(BenchmarkId::from_parameter(rules), &sheet, |b, sheet| {
            b.iter(|| {
                black_box(easybook::rewrite::css(
                    black_box(sheet),
                    "OEBPS/Styles",
                    "Serie 01/T01.epub",
                    "/api/asset",
                ))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, rewrite_html, rewrite_css);
criterion_main!(benches);
