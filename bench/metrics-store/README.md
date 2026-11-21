# Banc d'essai : stockage de l'historique de performances

Compare des moteurs **embarqués** pour conserver 30 jours de mesures RAM/CPU.
Résultats et décision : [`../reports/metrics-store.md`](../reports/metrics-store.md).

```bash
# moteurs par défaut : SQLite (2 variantes), redb, LMDB
cargo run --release -- --engine sqlite-blob
cargo run --release -- --engine sqlite-wal
cargo run --release -- --engine redb
cargo run --release -- --engine lmdb

# DuckDB (compilation C++ longue la première fois)
cargo run --release --no-default-features --features duckdb -- --engine duckdb
```

Options : `--days 30`, `--interval 10` (secondes), `--writes 500`,
`--batch 1000`, `--dir <dossier>`, `--no-trim`, `--list`.

Chaque moteur reçoit la même charge et rend les mêmes agrégats, comparés à un
calcul de référence en Rust (`mismatches: 0` attendu). Les bases sont créées
sous `.data/`, jamais versionné.
