# Cache des pages : JSON actuel contre bases embarquées

Date : 2026-11-19. Branche de départ : `master` (0.3.4). Profil **dev** partout.

Question : le cache des comptages de pages (`pages.json`, voir `backend/src/pages.rs`) gagnerait-il à passer sur une base embarquée ?

## Méthode

- **Charge** : clé = chemin d'environ 110 octets, valeur = 3 entiers (taille, date, nombre de pages) = 24 octets. Clés insérées et relues dans un ordre pseudo-aléatoire. Chaque lookup est comparé à la valeur attendue (`bad=0` partout).
- **JSON (actuel)** : `HashMap` derrière un `Mutex`, fichier relu en entier au démarrage et réécrit en entier à chaque sauvegarde (écriture dans un `.tmp` puis renommage).
- **Moteurs** : SQLite (`rusqlite` embarqué, 3 réglages de journal), LMDB (`heed`), RocksDB, sled, redb, DuckDB (bibliothèque précompilée, défaut et `PRAGMA threads=1`).
- **Profil** : dev, dépendances en `opt-level 3`, code du bench en `opt-level 2`, comme `backend/Cargo.toml`.
- **Répétitions** : médianes de 5 runs d'ouverture et de lecture, 3 runs pour les écritures et le remplissage. Un process par mesure.
- **Écritures durables** : remplissage en une transaction ; RocksDB avec `sync=true` ; sled par lots de 10 000 puis `flush`. Les lookups, les écritures et les 4 threads sont mesurés sur une base déjà remplie.
- **DuckDB** : 5 000 lookups au lieu de 200 000 (2 500 par thread en multi-thread), pour limiter le temps CPU. Les débits sont ramenés à l'opération.
- **Limites** : cache disque de l'OS toujours chaud, disque virtuel WSL2, RAM de base du process (27,4 Mo, `libduckdb` chargée) déduite de « RAM après ouverture ».

## Lecture des colonnes

| Colonne | Signification | Sens |
|---|---|---|
| Remplissage | Insertion de toutes les entrées en une transaction durable | bas = mieux |
| Disque | Taille apparente des fichiers de la base après remplissage | bas = mieux |
| Ouverture | Ouverture de la base au démarrage de l'API (pour le JSON : lecture du fichier et reconstruction du `HashMap`) | bas = mieux |
| RAM après ouverture | RAM résidente du process juste après l'ouverture, moins la RAM de base | bas = mieux |
| Lookup | Temps moyen pour retrouver le comptage d'un livre par son chemin (un par livre à chaque scan) | bas = mieux |
| Lookup en lot | Idem, dans une seule transaction de lecture | bas = mieux |
| +100 entrées | Ajout de 100 livres nouveaux, écriture durable comprise | bas = mieux |
| 1 écriture | Ajout d'un livre, une transaction par livre (médiane et p99 sur 200 écritures ; 5 pour le JSON à 1 M) | bas = mieux |
| 4 threads | Débit de lookups avec 4 threads simultanés, en milliers par seconde | **haut = mieux** |

## Résultats

### 1 100 entrées (ordre de grandeur de la production)

| Moteur | Remplissage (ms) | Disque (Mo) | Ouverture (ms) | RAM après ouverture (Mo) | Lookup (ns) | Lookup en lot (ns) | +100 entrées (ms) | 1 écriture méd. (ms) | 1 écriture p99 (ms) | 4 threads (k lookups/s) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| JSON (actuel) | 0,7 | 0,2 | 0,5 | 0,0 | 52 | 54 | 0,83 | 0,542 | 1,828 | 4 963 |
| SQLite défaut | 6,7 | 0,2 | 0,3 | 1,0 | 3 367 | 3 310 | 5,12 | 3,772 | 11,243 | 836 |
| SQLite WAL | 1,0 | 0,2 | 0,7 | 1,1 | 1 585 | 1 570 | 1,84 | 0,013 | 0,099 | 750 |
| SQLite WAL FULL | 3,3 | 0,2 | 0,6 | 0,9 | 1 528 | 1 559 | 4,18 | 1,185 | 3,438 | 792 |
| LMDB | 5,4 | 0,3 | 0,1 | 0,0 | 207 | 145 | 2,32 | 0,847 | 11,115 | 12 677 |
| RocksDB | 12,7 | 0,2 | 22,7 | 1,0 | 816 | 793 | 4,30 | 0,976 | 2,687 | 2 411 |
| sled | 5,8 | 0,5 | 5,2 | 1,0 | 285 | 276 | 1,43 | 0,360 | 0,802 | 6 288 |
| redb | 6,3 | 0,3 | 2,2 | 0,2 | 672 | 191 | 2,95 | 0,619 | 2,289 | 1 550 |
| DuckDB | 10,9 | 0,8 | 28,9 | 31,2 | 405 413 | 406 158 | 8,26 | 3,484 | 8,294 | 6 |
| DuckDB 1 thread | 11,1 | 0,8 | 35,4 | 31,5 | 313 920 | 311 599 | 7,36 | 2,571 | 6,835 | 8 |

### 6 000 entrées

| Moteur | Remplissage (ms) | Disque (Mo) | Ouverture (ms) | RAM après ouverture (Mo) | Lookup (ns) | Lookup en lot (ns) | +100 entrées (ms) | 1 écriture méd. (ms) | 1 écriture p99 (ms) | 4 threads (k lookups/s) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| JSON (actuel) | 6,0 | 0,9 | 2,5 | 0,5 | 124 | 101 | 6,37 | 2,772 | 5,648 | 1 997 |
| SQLite défaut | 16,0 | 0,8 | 0,4 | 0,9 | 4 423 | 4 144 | 7,12 | 3,921 | 8,045 | 798 |
| SQLite WAL | 9,7 | 0,8 | 0,5 | 1,1 | 1 983 | 1 917 | 2,68 | 0,009 | 0,127 | 437 |
| SQLite WAL FULL | 17,2 | 0,8 | 0,8 | 1,1 | 2 910 | 2 568 | 3,89 | 1,194 | 3,708 | 593 |
| LMDB | 10,4 | 1,2 | 0,1 | 0,0 | 299 | 226 | 3,61 | 0,975 | 7,739 | 10 581 |
| RocksDB | 9,7 | 0,8 | 21,5 | 1,1 | 951 | 904 | 3,59 | 1,057 | 5,293 | 2 469 |
| sled | 80,5 | 6,4 | 10,6 | 1,4 | 661 | 568 | 2,11 | 0,590 | 1,006 | 3 601 |
| redb | 23,0 | 1,1 | 2,6 | 0,2 | 1 000 | 287 | 2,76 | 0,537 | 2,963 | 1 436 |
| DuckDB | 25,5 | 1,3 | 38,5 | 31,4 | 591 700 | 583 265 | 10,41 | 3,639 | 7,549 | 5 |
| DuckDB 1 thread | 41,9 | 1,3 | 37,8 | 31,4 | 362 519 | 387 443 | 8,65 | 2,677 | 6,339 | 9 |

### 100 000 entrées

| Moteur | Remplissage (ms) | Disque (Mo) | Ouverture (ms) | RAM après ouverture (Mo) | Lookup (ns) | Lookup en lot (ns) | +100 entrées (ms) | 1 écriture méd. (ms) | 1 écriture p99 (ms) | 4 threads (k lookups/s) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| JSON (actuel) | 131,5 | 15,5 | 58,0 | 17,7 | 447 | 388 | 84,59 | 38,256 | 74,136 | 867 |
| SQLite défaut | 414,0 | 13,2 | 0,5 | 1,1 | 7 445 | 6 588 | 5,61 | 5,550 | 11,882 | 459 |
| SQLite WAL | 627,6 | 13,2 | 0,5 | 1,0 | 4 178 | 3 734 | 2,31 | 0,013 | 0,109 | 578 |
| SQLite WAL FULL | 564,6 | 13,2 | 0,5 | 1,1 | 3 268 | 3 091 | 3,11 | 1,249 | 3,517 | 626 |
| LMDB | 282,3 | 22,1 | 0,1 | 0,0 | 804 | 667 | 3,55 | 1,036 | 2,988 | 4 097 |
| RocksDB | 164,8 | 12,6 | 16,9 | 1,4 | 1 531 | 1 719 | 2,49 | 1,766 | 3,928 | 1 603 |
| sled | 1 673,7 | 64,5 | 25,7 | 6,1 | 1 719 | 1 461 | 1,91 | 0,430 | 1,943 | 1 732 |
| redb | 225,7 | 32,9 | 1,9 | 0,4 | 1 609 | 954 | 1,60 | 0,586 | 11,592 | 980 |
| DuckDB | 239,1 | 11,8 | 30,8 | 31,4 | 519 348 | 502 821 | 10,40 | 3,080 | 6,335 | 4 |
| DuckDB 1 thread | 222,4 | 11,8 | 29,9 | 31,6 | 384 128 | 349 636 | 9,15 | 2,775 | 11,446 | 8 |

### 1 000 000 d'entrées

Remplissage en secondes.

| Moteur | Remplissage (s) | Disque (Mo) | Ouverture (ms) | RAM après ouverture (Mo) | Lookup (ns) | Lookup en lot (ns) | +100 entrées (ms) | 1 écriture méd. (ms) | 1 écriture p99 (ms) | 4 threads (k lookups/s) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| JSON (actuel) | 1,6 | 156,1 | 622,5 | 219,7 | 799 | 785 | 596,01 | 535,093 | 753,546 | 509 |
| SQLite défaut | 6,4 | 139,7 | 0,3 | 0,9 | 7 033 | 6 361 | 4,89 | 3,700 | 10,005 | 383 |
| SQLite WAL | 8,6 | 139,7 | 0,5 | 1,1 | 3 755 | 3 997 | 3,74 | 0,007 | 0,106 | 549 |
| SQLite WAL FULL | 8,3 | 139,7 | 0,5 | 1,1 | 3 524 | 3 440 | 2,92 | 1,134 | 3,251 | 601 |
| LMDB | 1,6 | 213,1 | 0,1 | 0,0 | 1 130 | 1 007 | 3,55 | 0,827 | 3,668 | 2 631 |
| RocksDB | 2,2 | 126,0 | 19,0 | 2,3 | 1 748 | 1 553 | 2,73 | 1,004 | 3,009 | 920 |
| sled | 15,4 | 594,6 | 104,6 | 37,4 | 3 276 | 2 319 | 2,27 | 0,343 | 21,827 | 874 |
| redb | 2,4 | 272,0 | 3,0 | 0,2 | 1 992 | 1 304 | 0,84 | 0,582 | 2,370 | 921 |
| DuckDB | 2,4 | 113,8 | 27,4 | 31,8 | 432 998 | 407 981 | 8,74 | 2,331 | 9,401 | 6 |
| DuckDB 1 thread | 2,3 | 113,5 | 26,4 | 31,8 | 291 939 | 268 515 | 8,04 | 1,748 | 4,116 | 9 |

## Test de bout en bout : JSON contre SQLite

Mesuré avant le bench ci-dessus, en remplaçant réellement `pages.json` par SQLite (journal par défaut, une transaction par scan) sur la branche `exp/sqlite-pages`, non commitée. Le test appelle `library::scan` sur des bibliothèques de CBZ synthétiques, en profil dev. Médianes de 3 runs à froid et 5 runs à chaud.

| Mesure | 60 livres | 6 000 | 50 000 | 100 000 |
|---|--:|--:|--:|--:|
| Pic de RAM après chargement (SQLite / JSON) | 1,32× | 1,13× | 0,52× | 0,31× |
| Chargement au démarrage | 8× (0,8 ms) | 0,73× | 0,67× | 0,62× |
| Re-scan, tout en cache | 3× | 2,3× | 2,2× | 2,0× |
| Lookup d'un comptage | 3,8 µs contre 0,09 µs | 29× | 13× | 11× |
| Scan à froid, CPU | 1,12× | 1,12× | 1,32× | 1,36× |
| +100 livres, scan | 4× | 1,2× | 1,35× | 1,8× |
| Taille sur disque | 2,5× (16 Ko) | 0,85× | 0,83× | 0,85× |

Le binaire grossit de 2,1 Mo avec `rusqlite` embarqué (49,7 Mo contre 51,8 Mo en dev). Les 123 tests de la branche passaient.

## Constats

- **À 1 100 entrées**, le JSON est le plus rapide en lookup (52 ns), en ajout de 100 livres (0,83 ms) et en remplissage. Toutes ses mesures restent sous la milliseconde. Seuls LMDB (débit à 4 threads : 12 677 contre 4 963 k lookups/s) et SQLite WAL (écriture unitaire : 0,013 ms contre 0,542 ms) font mieux, sans effet visible pour 1 100 livres.
- **À 100 000 entrées**, le JSON perd sur l'ouverture (58 ms), la RAM (17,7 Mo), l'écriture (38 ms par livre, 85 ms pour 100) et le débit concurrent. LMDB bat le JSON sur tout, sauf le lookup mono-thread (804 contre 447 ns) et le disque (22,1 contre 15,5 Mo).
- **À 1 000 000**, le JSON charge 220 Mo, ouvre en 622 ms et réécrit 535 ms par livre ajouté. Les autres moteurs écrivent en 0,3 à 3,7 ms.
- **SQLite** : lookup de 1,5 à 7 µs, soit 5 à 30 fois plus lent que le `HashMap`. Le mode WAL ramène une écriture unitaire de 3,7 ms à 7 à 13 µs, quelle que soit la taille.
- **LMDB** : meilleur moteur pour les lectures concurrentes et l'ouverture. Contreparties : fichier 1,3 à 1,4× plus gros que le JSON, taille de mapping à fixer à l'ouverture (8 Go dans le bench), ouverture `unsafe` côté `heed`, RAM résidente qui croît avec les lectures (258 Mo de pic à 1 M, via `mmap`, récupérable par l'OS).
- **RocksDB** : pas d'avantage ici, 17 à 23 ms d'ouverture et 4 threads de fond. Build C++ long, `libclang` requis.
- **sled** : remplissage de 15,4 s à 1 M et fichier 4× plus gros (595 Mo). Le crate est resté en 0.34 bêta.
- **redb** : correct partout, meilleur ajout de 100 entrées à 1 M (0,84 ms), mais 272 Mo sur disque.
- **DuckDB** : lookups de 0,3 à 0,6 ms, 6 à 9 k lookups/s à 4 threads, une soixantaine de threads créés par défaut, 31 Mo de RAM dès l'ouverture, bibliothèque de 96 Mo. Pas adapté aux accès par clé.

## Conclusion

- **À la taille de production visée (environ 1 100 livres)**, aucun moteur n'apporte de gain visible. Le coût d'un scan complet en lookups est d'environ 60 µs avec le JSON et 230 µs avec LMDB.
- **Seuil de bascule** : le JSON devient un vrai problème vers 100 000 livres. Si cela arrive, LMDB est le meilleur candidat pour ce cache, redb une alternative sans dépendance C.
- **Comptes utilisateurs** (non mesurés ici) : SQLite en mode WAL, avec `synchronous=NORMAL`, dans un fichier séparé du cache. Les comptes demandent des contraintes d'unicité et des requêtes relationnelles que les bases clé-valeur n'offrent pas.

## Reproduction

Les sources et résultats bruts sont dans `~/.eb_bench/` (hors dépôt) :

- `dbbench/` : projet Cargo du bench (profil dev), moteur et phase en arguments : `dbbench <moteur> <fill|open|incr|single|mt> <N> <rep> <dossier>`.
- `run_db.py` : pilote complet, journal d'avancement dans `progress.log`.
- `db_results.json` (6 000, 100 000, 1 000 000) et `db_results_1100.json` : résultats bruts.
- `bench_pages.rs`, `run_pages.py`, `pages_results.json` : test de bout en bout JSON contre SQLite.

DuckDB et RocksDB ne se compilent pas dans les mêmes conditions que le reste : DuckDB utilise `libduckdb.so` précompilée (`download-lib`), RocksDB a besoin de `libclang` (installé hors système dans `~/.eb_bench/libclang`).
