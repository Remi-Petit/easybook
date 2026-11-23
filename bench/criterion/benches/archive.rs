//! Ouverture des archives et lecture d'une entrée : le coût fixe de chaque
//! requête de page (`/api/page`, `/api/asset`, chapitre EPUB).
//!
//! L'archive est servie par un **réservoir** (`archive::pooled`) : une requête
//! retombe sur le descripteur déjà ouvert et n'a plus qu'à chercher l'entrée
//! dans le répertoire central. Les deux étapes sont mesurées séparément, pour
//! que l'on sache laquelle payerait une régression.

mod common;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use easybook::archive;
use std::hint::black_box;

fn open_and_read(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("dossier temporaire");
    let path = dir.path().join("Serie 01").join("T01.cbz");
    // 80 planches : la taille d'un tome, donc un répertoire central réaliste.
    common::cbz(&path, 80);
    // Première ouverture hors mesure : elle remplit le réservoir, et c'est son
    // état chaud que voient les requêtes suivantes.
    drop(archive::open(&path).expect("ouverture de la fixture"));
    assert_eq!(archive::pooled(), 1);

    let mut group = c.benchmark_group("archive");
    group.bench_function("open_pooled", |b| {
        b.iter(|| black_box(archive::open(black_box(&path)).expect("archive ouverte")));
    });
    group.bench_function("read_page", |b| {
        b.iter(|| {
            let mut zip = archive::open(black_box(&path)).expect("archive ouverte");
            black_box(archive::read(&mut zip, "page-042.jpg"))
        });
    });
    group.finish();

    // Lecture par nom tolérant (`EPUB`) : trois essais possibles avant de
    // parcourir tous les noms de l'archive. La casse exacte montre le coût
    // d'une lecture réussie, la variante `Page-042.JPG` celui de la recherche
    // insensible à la casse. `absente.jpg` échoue : elle ne paie que le
    // parcours des noms (quelques microsecondes), pas l'inflation de l'entrée
    // — c'est ce qui explique qu'une absence soit moins chère qu'une lecture.
    let mut group = c.benchmark_group("archive/read_named");
    for name in ["page-042.jpg", "Page-042.JPG", "absente.jpg"] {
        group.bench_with_input(BenchmarkId::new("entry", name), name, |b, name| {
            b.iter(|| {
                let mut zip = archive::open(black_box(&path)).expect("archive ouverte");
                black_box(archive::read_named(&mut zip, black_box(name)))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, open_and_read);
criterion_main!(benches);
