# Micro-benchmarks CPU (criterion)

Mesure le coût **in-process** des fonctions qui portent le CPU d'easybook :
réécriture des documents, réduction des planches, ouverture d'archives, parcours
du catalogue. C'est le complément de [`../k6`](../k6) : k6 mesure le serveur de
bout en bout (nginx, HTTP, Tokio, disque), criterion mesure le code.

```bash
cd bench/criterion

cargo bench                              # toutes les familles (~2-3 min)
cargo bench --bench resize               # une famille
cargo bench -- resize/shrink_page        # un filtre par nom
cargo bench -- --quick                   # 10 échantillons : premier jet
```

La première compilation prend quelques minutes : le crate dépend de `backend`
(toute la chaîne d'`easybook`, `unrar` compris). Le rapport HTML est écrit dans
`target/criterion/report/index.html`.

## Pourquoi criterion et pas un `Instant::now()`

Une mesure isolée ne dit rien : la machine breathe, le cache se réchauffe, le CPU
change de fréquence. Criterion répète l'appel pendant un temps de chauffe, prend
des dizaines d'échantillons et rend une **médiane avec son intervalle de
confiance**. Surtout, il sait **comparer deux runs** et dire si l'écart est réel :

```bash
git checkout master     && cargo bench -- --save-baseline master
git checkout ma-branche && cargo bench -- --baseline master
```

```
resize/shrink_page/page/1200x1884
                        time:   [27.412 ms 27.930 ms 28.446 ms]
                        change: [-13.3% -11.7% -10.1%] (p = 0.00 < 0.05)
                        Performance has improved.
```

`p < 0.05` : l'écart n'est pas du bruit. « No change in performance detected »
veut dire que le gain espéré n'est pas là, pas que le run a échoué.

## Lire un résultat

| Champ | Sens |
| --- | --- |
| `time: [bas médiane haut]` | intervalle de confiance à 95 % du temps par itération |
| `thrpt` | débit : MiB/s pour les images, éléments/s pour les parcours |
| `change` | écart avec la référence, en % et en valeur absolue |
| `p` | probabilité que l'écart observé soit du bruit |

## Et le Big O ?

Criterion **ne calcule pas** la complexité : il la **vérifie**. Les bancs sont
paramétrés (`BenchmarkId::from_parameter`) pour qu'une entrée dix fois plus
grosse doive coûter dix fois plus cher :

| `rewrite/html` | 50 paragraphes | 500 | 5 000 |
| --- | --- | --- | --- |
| temps | ~180 µs | ~1,9 ms | ~16,5 ms |

Le rapport est linéaire en la taille du document — pas de recherche linéaire
cachée. Un facteur qui s'envole (10× d'entrée, 100× de temps) signale une
complexité en $O(n^2)$ : c'est exactement ce qu'un outil comme
[bigocalc](https://www.bigocalc.com/) *estime* sur un extrait de code, et que ce
banc *mesure* sur le vrai code compilé.

Les deux sont complémentaires : le Big O ignore les constantes, criterion les
mesure — deux algorithmes en $O(n)$ peuvent différer d'un facteur dix, et c'est
souvent cette constante qui décide du ressenti.

## Ce que ces bancs ne mesurent pas

- le disque **froid** et le cache de page du système (`library` mesure un scan à
  cache chaud : le second, celui d'un rechargement du catalogue) ;
- nginx, HTTP/1.1-2, TLS, la concurrence, Tokio, la RAM : c'est le rôle de k6 ;
- le rendu frontend : `bench/k6` scénario `ui`.

Pour des chiffres utilisables, la machine doit être au repos : pas de
`docker compose up` ni de `cargo build` en parallèle, sur secteur.

## Les familles

| Fichier | Mesure | À surveiller |
| --- | --- | --- |
| `benches/rewrite.rs` | `rewrite::html` / `rewrite::css` sur un chapitre et une feuille de style | croissance linéaire avec la taille du document |
| `benches/resize.rs` | `resize::shrink_page` : décodage JPEG, SIMD, ré-encodage | le chemin `1900 -> 800` (décodage direct à l'échelle 1/4) doit battre le `1200 -> 800` à pixels égaux |
| `benches/archive.rs` | `archive::open` (réservoir chaud), `read`, `read_named` | le coût fixe par requête de page |
| `benches/library.rs` | `library::scan` sur 100 et 1 000 livres | croissance linéaire avec le nombre de livres |

Les fixtures sont **générées** à la volée (`benches/common/mod.rs`) : aucune
planche binaire au dépôt, mêmes entrées à chaque run. Les images sont des
dégradés bruités de taille réaliste (≈ 650 Ko pour 1200 x 1884), pas des aplats
qui se décodent trop vite.

## Ajouter un banc

1. Écrire `benches/mon_sujet.rs` :

   ```rust
   mod common;

   use criterion::{Criterion, criterion_group, criterion_main};
   use std::hint::black_box;

   fn mon_banc(c: &mut Criterion) {
       let entree = common::chapter(500);
       c.bench_function("mon/sujet", |b| {
           b.iter(|| black_box(easybook::rewrite::html(black_box(&entree), "OEBPS/Text", "a.epub", "/api/asset")))
       });
   }

   criterion_group!(benches, mon_banc);
   criterion_main!(benches);
   ```

2. Déclarer la cible dans `Cargo.toml` — sans `harness = false`, le harnais de
   test de Rust remplacerait celui de criterion :

   ```toml
   [[bench]]
   name = "mon_sujet"
   path = "benches/mon_sujet.rs"
   harness = false
   ```

3. Si la fixture manque, l'ajouter à `common/mod.rs` (avec `#![allow(dead_code)]`
   déjà présent, les autres bancs ne s'en plaindront pas).

## En CI (facultatif)

Les runners GitHub sont partagés et bruités : un seuil absolu échouerait au
hasard. La comparaison utile est **deux runs dans le même job, avec le même
`target/`** — une référence prise sur le code de base, puis le code de la PR.
Criterion range ses références dans `target/criterion`, donc les deux mesures
doivent partager le dossier de compilation :

```yaml
- name: Bancs (comparaison avec la base)
  run: |
    cd bench/criterion
    git checkout ${{ github.event.pull_request.base.sha }} -- ../../backend ../../bench/criterion/benches
    cargo bench -- --save-baseline base
    git checkout ${{ github.event.pull_request.head.sha }} -- ../../backend ../../bench/criterion/benches
    cargo bench -- --baseline base
```

Les chiffres restent un indicateur, pas une porte bloquante (contrairement à
`cargo test`, qui lui est déterministe).
