//! Parcours du dossier des livres (`library::scan`) : ce que paie le serveur à
//! chaque rechargement du catalogue — et, en développement, à chaque
//! modification du dossier surveillé.
//!
//! Le banc compare 100 et 1 000 livres : le second doit coûter dix fois le
//! premier (parcours + tri + comptage récursif sont linéaires). Le **premier**
//! scan, lui, ouvre chaque livre pour compter ses pages ; il est fait une fois
//! hors mesure, car le cache des comptages (`pages`) vit ensuite en mémoire pour
//! tout le process. Les chiffres ci-dessous mesurent donc un scan à cache chaud,
//! qui est le cas d'un rechargement — pas celui d'un démarrage à froid.

mod common;

use criterion::{
    BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use easybook::library;
use std::hint::black_box;

fn scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("library/scan");
    // Un scan de 1 000 livres dure des centaines de millisecondes : peu
    // d'échantillons, et `Flat` pour ne pas doubler le temps par des tailles
    // d'entrée interpolées qui n'ont pas de sens ici.
    group.sample_size(10);
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    for books in [100usize, 1_000] {
        let dir = tempfile::tempdir().expect("dossier temporaire");
        let root = dir.path().join("books");
        common::tree(&root, books);
        // Scan hors mesure : il ouvre chaque livre et remplit le cache des
        // comptages de pages.
        black_box(library::scan(&root));

        group.bench_with_input(BenchmarkId::from_parameter(books), &root, |b, root| {
            b.iter(|| black_box(library::scan(black_box(root))));
        });
    }
    group.finish();
}

criterion_group!(benches, scan);
criterion_main!(benches);
