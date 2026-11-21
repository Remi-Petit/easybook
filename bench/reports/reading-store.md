# Historique de lecture : choix du stockage

Date : 2026-11-21. Branche : `feat/reading-history`.
Machine : Windows 11, 16 processeurs logiques, disque local, profil `release`.

Question : où enregistrer **l'historique de lecture par utilisateur** (position
courante dans chaque livre + sessions), sachant qu'easybook reste une **image
unique** et que la charge est l'inverse de celle de l'historique de
performances : peu de lignes, beaucoup d'**écritures aléatoires en place**.

## Méthode

- **Charge** : 20 utilisateurs × 400 livres lus = **8 000 positions** et
  **8 000 sessions** (un lecteur régulier sur une bibliothèque de 2 000 livres).
  Puis **3 000 écritures chaudes** : une position qui avance d'une page et sa
  session touchée — c'est le chemin d'un changement de page dans le lecteur.
- **Lectures** : les trois requêtes de la page d'administration — les 50 livres
  les plus récemment lus d'un utilisateur, les totaux (livres commencés /
  terminés / pages), les 100 dernières sessions. Médiane de 5 passes.
- **Durabilité comparable** : SQLite en `journal_mode = WAL` +
  `synchronous = NORMAL`, redb en `Durability::Eventual` — un crash applicatif
  ne perd rien, une coupure de courant peut perdre les dernières écritures
  (même compromis que `accounts.db` et `metrics.db`).
- **Validation** : chaque moteur doit rendre exactement les mêmes réponses
  qu'un calcul de référence en Rust (`mismatches: 0` exigé).

## Résultats

| Variante | Ouverture | Remplissage | Écrit. p50 / p99 | Dernier lu | Totaux | Sessions | Disque | RAM |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| **SQLite WAL, 2 tables** | 19 ms | **716 ms** | **37 µs / 106 µs** | 60 µs | 54 µs | 77 µs | **1,7 Mo** | **9,9 Mo** |
| SQLite, 1 ligne/utilisateur (JSON) | 17 ms | 3,0 s | 240 µs / 412 µs | 195 µs | 135 µs | 74 µs | 1,3 Mo | 10,5 Mo |
| redb | 14 ms | 36 s | 4,5 ms / 5,4 ms | **16 µs** | 40 µs | 36 µs | 5,6 Mo | 12,7 Mo |
| DuckDB | 141 ms | 102 s | 9,1 ms / 10,3 ms | 2,3 ms | 2,0 ms | 1,5 ms | 5,8 Mo | 80,5 Mo |

### Ce que disent les chiffres

- **L'upsert SQLite est imbattable ici** : 37 µs par écriture, à comparer aux
  4,5 ms de redb (chaque commit met à jour deux index B-tree et son journal) et
  aux 9,1 ms de DuckDB (moteur colonnaire, fait pour les balayages, pas pour
  réécrire une ligne).
- **Le JSON par utilisateur est un piège** : réécrire tout le document à chaque
  changement de page coûte 6 fois plus cher à l'écriture, et la lecture doit
  reparser le JSON (×3). Il ne gagne que 0,4 Mo de disque.
- **redb gagne les lectures** (16 µs pour le tri, l'index étant une clé
  préfixée) — mais 16 µs contre 60 µs ne change rien à l'échelle d'une page
  d'administration, alors que 4,5 ms contre 37 µs se voit sur le chemin chaud.
- **DuckDB paie son binaire** : 80 Mo de RAM résidente et un démarrage à 141 ms
  pour un usage « une ligne à la fois » qui n'est pas le sien.

## Décision : SQLite (WAL), deux tables

1. **Déjà là** : `rusqlite` est une dépendance existante, la même image, le même
   volume `/data` — aucune surcharge pour un historique aussi petit.
2. **Le chemin chaud est le meilleur** : la position s'écrit en 37 µs, très en
   dessous du temps de rendu d'une page.
3. **Les lectures de la page admin sont instantanées** (60 µs), avec un index
   `(user_id, updated_at DESC)` qui sert exactement le tri demandé.
4. **Simplicité** : deux tables, un fichier, une transaction par écriture ; pas
   d'index à maintenir à la main comme en clé-valeur.
5. **Cohérence** : même schéma, même réglage WAL, même dossier que
   `accounts.db` et `metrics.db` — une seule façon de faire dans le projet.

## Schéma retenu

```sql
CREATE TABLE reading_progress(
    user_id    TEXT NOT NULL,          -- users.id (UUID)
    book       TEXT NOT NULL,          -- chemin relatif, comme le catalogue
    title      TEXT NOT NULL,          -- gardé même si le livre disparaît
    format     TEXT NOT NULL,
    page       INTEGER NOT NULL,       -- index 0-based (page ou chapitre)
    pages      INTEGER NOT NULL,       -- total connu au moment de la lecture
    started_at INTEGER NOT NULL,       -- première ouverture
    updated_at INTEGER NOT NULL,       -- dernière position
    PRIMARY KEY (user_id, book)
) WITHOUT ROWID;

CREATE INDEX reading_recent ON reading_progress(user_id, updated_at DESC);

CREATE TABLE reading_sessions(
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    TEXT NOT NULL,
    book       TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    last_at    INTEGER NOT NULL,
    end_page   INTEGER NOT NULL,
    pages      INTEGER NOT NULL
);

CREATE INDEX reading_sessions_user ON reading_sessions(user_id, last_at DESC);
```

Une session est **poursuivie** tant que la même paire (utilisateur, livre) écrit
à moins de 60 s d'intervalle — le même seuil que `readings::IDLE`, qui décide
déjà quand une lecture est terminée côté serveur. Aucun identifiant de session
n'a donc besoin de transiter par le client.

## Reproductibilité

```bash
cd bench/metrics-store
cargo run --release --bin reading-store-bench -- --engine sqlite
cargo run --release --bin reading-store-bench -- --engine sqlite-blob
cargo run --release --bin reading-store-bench -- --engine redb
cargo run --release --features duckdb --bin reading-store-bench -- --engine duckdb
# autres volumes : --users 50 --books 5000 --reads 800 --writes 10000
```
