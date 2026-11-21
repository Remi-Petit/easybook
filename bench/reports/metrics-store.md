# Historique des performances : choix du stockage

Date : 2026-11-21. Branche : `feat/perf-history` (depuis `feat/admin-reset`).
Machine : Windows 11, 16 processeurs logiques, disque local, profil `release`.

Question : où enregistrer les mesures RAM/CPU de la page Performances pour
conserver **30 jours** d'historique, sachant que le serveur doit rester une
**image unique** (pas de service externe) ?

## Méthode

- **Charge** : 30 jours à une mesure toutes les 10 s = **259 200 mesures**, soit
  259 200 lignes si les quatre groupes de programmes tiennent dans la ligne, ou
  **1 296 000 lignes** si les groupes sont normalisés (une ligne par groupe).
  Les valeurs imitent une vraie journée : marée de ~2 h, bruit déterministe,
  mémoire et CPU réalistes (API 35 Mo, interface 62 Mo, nginx 9 Mo,
  superviseur 3 Mo).
- **Écritures** : remplissage par lots de 1 000 mesures (une transaction
  durable par lot), puis **500 écritures unitaires** (une mesure = 5 lignes en
  base normalisée, 1 ligne en base compacte), comme le fera l'échantillonneur.
- **Lectures** : lecture brute des 3 dernières minutes ; agrégats de 24 h (pas
  de 1 min), 7 j (10 min), 30 j (1 h), et **tous les groupes sur 30 j** (ce
  qu'affiche la courbe). Médiane de 5 passes.
- **Purge** : suppression d'un jour de mesures, comme le balayage quotidien.
- **Validation** : chaque agrégat rendu par chaque moteur est comparé à un
  calcul de référence en Rust (`bad=0` exigé). Les chiffres ci-dessous n'ont de
  sens que si les résultats sont identiques.
- **RAM** : mémoire résidente du process après libération de la charge de test.
- **Disque** : tous les fichiers de la base, journal compris, après
  point de contrôle.

Les mesures sont relatives (même machine, même charge, même jour). Les valeurs
absolues sous Linux dans le conteneur seront différentes ; le **classement** et
les ordres de grandeur sont ce qui compte.

## Résultats (30 jours)

| Variante | Ouverture | Remplissage | Écrit. p50 / p99 | 3 min | 24 h | 7 j | 30 j | 30 j tous groupes | Purge 1 j | Disque | RAM |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| SQLite WAL, 2 tables normalisées | 17 ms | 1,62 s | 53 µs / 2,1 ms | 5 µs | 2,3 ms | 19,7 ms | 83 ms | **458 ms** | 28 ms | 68,7 Mo | **9,0 Mo** |
| **SQLite WAL, 1 ligne/mesure + BLOB** | 17 ms | **326 ms** | **12 µs / 66 µs** | 5 µs | 2,5 ms | 22,7 ms | 96 ms | **82 ms** | 13 ms | **24,2 Mo** | **8,6 Mo** |
| redb | 13 ms | 2,50 s | 2,5 ms / 3,2 ms | 3 µs | 0,70 ms | 4,9 ms | 20,7 ms | 72 ms | 130 ms | 129 Mo | 84 Mo |
| LMDB (`heed`) | **1,4 ms** | 2,07 s | 0,22 ms / 0,35 ms | **1,4 µs** | **0,52 ms** | **2,7 ms** | 11,6 ms | 45 ms | 32 ms | 64,6 Mo | 70 Mo |
| DuckDB (embarqué) | 41 ms | 3,36 s | 5,6 ms / 6,4 ms | 1,46 ms | 3,9 ms | 5,0 ms | **6,9 ms** | **10,5 ms** | 102 ms | 100 Mo | 55 Mo |

### Pièges rencontrés (et vérifiés)

- **DuckDB arrondit un `CAST(DOUBLE AS BIGINT)`** : `497963,5` devient `497964`.
  Les tranches horaires calculées avec `CAST(at_ms / 3600000 AS BIGINT)`
  décalaient des lignes d'une heure en heure (3 892 agrégats faux sur 3 892).
  La division entière `at_ms // 3600000` corrige le problème.
- SQLite avec deux tables et `GROUP BY name, bucket` sur 1,3 million de lignes
  coûte 458 ms : un index couvrant ne gagne que 5 % et fait grossir le fichier
  de 15 Mo. Le BLOB par mesure (agrégation en Rust sur 259 200 lignes) divise
  ce coût par 5,6 et le fichier par 2,8.
- LMDB n'accepte ni clé vide ni `CAST` implicite : une plage « du début à X »
  doit être non bornée à gauche, sinon `MDB_BAD_VALSIZE`.

## Décision : SQLite (WAL), une ligne par mesure, groupes en BLOB

1. **Déjà là** : `rusqlite` est déjà une dépendance (base des comptes), la même
   image, le même volume `/data`. Aucun binaire, service ou build C++ en plus
   (DuckDB ajoute ~38 Mo au binaire et ~55 Mo de RAM).
2. **Le travail réel est dérisoire** : une mesure toutes les 10 s = 0,1
   écriture/s. Même les 5,6 ms d'écriture de DuckDB suffiraient ; le critère
   n'est pas le débit d'écriture mais l'encombrement et la simplicité.
3. **La lecture qui compte est bonne** : 82 ms pour 30 jours et tous les
   groupes, moins que le rafraîchissement de 2 s de la page, et une réponse
   mise en cache quelques secondes côté serveur. Les 3 minutes (courbe en
   direct) restent dans la mémoire du navigateur, pas en base.
4. **Empreinte minimale** : 24 Mo pour 30 jours, 8,6 Mo de RAM, ouverture 17 ms.
5. **Un seul mécanisme** : une table, une purge, un point de contrôle WAL. Les
   moteurs clé-valeur imposeraient d'écrire l'agrégation à la main (et le
   schéma évolutif aussi) pour un gain nul à cette charge.
6. **DuckDB perd sur le coût** : 6 à 8 fois plus rapide en agrégat, mais 4 fois
   plus de disque, 6 fois plus de RAM, un binaire de 38 Mo et une compilation
   C++ dans l'image — pour une page d'administration ouverte rarement.

## Schéma retenu

```sql
CREATE TABLE perf_samples(
    at_ms      INTEGER PRIMARY KEY,   -- horodatage (ms)
    mem_total  INTEGER NOT NULL,      -- cgroup memory.current
    mem_anon   INTEGER NOT NULL,
    mem_file   INTEGER NOT NULL,
    mem_kernel INTEGER NOT NULL,
    cpu_pct    REAL    NOT NULL,      -- depuis la mesure précédente
    heap_live  INTEGER NOT NULL,      -- tas vivant de l'API
    sessions   INTEGER NOT NULL,
    sockets    INTEGER NOT NULL,
    groups     BLOB    NOT NULL       -- [id u16, rss u64, cpu f32] × n
) WITHOUT ROWID;

CREATE TABLE perf_programs(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
```

Le dictionnaire `perf_programs` garde les noms hors des lignes : le BLOB reste
compact (14 octets par groupe) et un programme inconnu (un worker nginx
supplémentaire, un shell) est ajouté à la volée.

## Reproductibilité

```bash
cd bench/metrics-store
cargo run --release -- --engine sqlite-blob
cargo run --release --no-default-features --features duckdb -- --engine duckdb
# autres : sqlite-wal, redb, lmdb
```

Le code du banc est dans `bench/metrics-store/` ; `--days`, `--interval`,
`--writes` et `--batch` permettent de rejouer d'autres volumes.
