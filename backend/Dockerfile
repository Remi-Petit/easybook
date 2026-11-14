# syntax=docker/dockerfile:1
#
# API easybook : scan de `books/`, lecture des livres, WebSocket temps réel.
# Le contexte de construction est la **racine** du dépôt (voir docker-compose.yml) :
# les chemins `backend/…` en découlent.

# ── 1. Compilation ─────────────────────────────────────────────────────────
FROM rust:1.98-slim-bookworm AS builder

WORKDIR /build/backend

# Les dépendances changent rarement : on les compile seules, à partir d'une
# crate vide. Modifier `src/` ne les recompile alors pas.
COPY backend/Cargo.toml ./
RUN mkdir -p src \
    && printf '' > src/lib.rs \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release

COPY backend/src ./src
# `touch` : les vraies sources doivent paraître plus récentes que la crate vide.
RUN touch src/lib.rs src/main.rs && cargo build --release

# ── 2. Exécution ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/backend/target/release/easybook /usr/local/bin/easybook

# Dans un conteneur, la publication de port ne relaie pas vers la boucle locale :
# l'API doit écouter sur toutes ses interfaces. Hors conteneur, le défaut reste
# `127.0.0.1` (l'API n'a pas d'authentification).
ENV PORT=8091 \
    EASYBOOK_HOST=0.0.0.0 \
    BOOKS_ROOT=/books

EXPOSE 8091

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8091/api/health || exit 1

CMD ["easybook"]
