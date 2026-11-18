# syntax=docker/dockerfile:1

# Image **unique** d'easybook : l'API Rust, l'interface SvelteKit et nginx
# tournent dans le même conteneur, supervisés par `scripts/docker-entrypoint.sh`
# (voir ce fichier pour le pourquoi du superviseur).
#
#   docker compose up --build                                  # développement (rapide)
#   docker compose -f docker-compose.release.yml up --build   # image de production
#   → interface + API : http://localhost:5200 (dev), :5300 (release)
#
# nginx est le seul point d'entrée : il sert les fichiers compilés du frontend,
# relaie `/api` et `/ws` vers l'API Rust, et le reste vers le serveur SvelteKit.
# Le navigateur ne connaît donc qu'une seule origine — pas de CORS, pas de
# second port à publier.

# Profil de compilation du backend : `release` (défaut : c'est l'image publiée et
# celle de la CI) ou `dev` (compilation incrémentale rapide, binaire non optimisé).
ARG BACKEND_PROFILE=release

# ── 1. Backend (Rust) ────────────────────────────────────────────────────
FROM rust:1.98-slim-bookworm AS backend-base

# `unrar` (format CBR) embarque les sources C++ d'unrar et les compile : il faut
# donc un compilateur C++ en plus de Rust.
RUN apt-get update \
    && apt-get install -y --no-install-recommends g++ make \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build/backend

# ── 1a. Compilation release : reproductible, optimisée, mise en cache par couches ──
FROM backend-base AS backend-release

# Les dépendances changent rarement : on les compile seules, à partir d'une
# crate vide. Modifier `src/` ne les recompile alors pas. `Cargo.lock` est
# versionné : la construction résout exactement les versions testées en CI.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir -p src \
    && printf '' > src/lib.rs \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release --locked

COPY backend/src ./src
# `touch` : les vraies sources doivent paraître plus récentes que la crate vide.
RUN touch src/lib.rs src/main.rs && cargo build --release --locked \
    && install -D target/release/easybook /out/easybook

# ── 1b. Compilation dev : incrémentale, pour l'aller-retour en local ──────────────
FROM backend-base AS backend-dev

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
# `target/` et le registre vivent dans des caches BuildKit, hors de l'image :
# modifier un fichier de `src/` ne recompile que easybook (les dépendances,
# optimisées par le profil dev de Cargo.toml, ne le sont qu'une fois). Le binaire
# est copié dans la même commande : le cache n'est pas visible à l'étape suivante.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/backend/target \
    cargo build --locked \
    && install -D target/debug/easybook /out/easybook

FROM backend-${BACKEND_PROFILE} AS backend

# ── 2. Frontend (SvelteKit, adapter-node) ──────────────────────────────────
FROM oven/bun:1.3.8-slim AS frontend

WORKDIR /build

# Lockfile versionné : installation reproductible, et la couche reste valide
# tant que les dépendances ne changent pas.
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
# `build/` est autonome : serveur compilé, client compilé, dépendances incluses.
RUN bun run build

# ── 3. Exécution : nginx + Node + API ──────────────────────────────────────
FROM node:22-bookworm-slim AS runtime

# nginx (le point d'entrée), `curl` pour le healthcheck. Debian bookworm fournit
# nginx 1.22, largement suffisant ici.
RUN apt-get update \
    && apt-get install -y --no-install-recommends nginx curl \
    && rm -rf /var/lib/apt/lists/* \
    && rm -f /etc/nginx/sites-enabled/default

WORKDIR /app

COPY --from=backend /out/easybook /usr/local/bin/easybook
COPY backend/config.yml /app/config.yml
COPY --from=frontend --chown=node:node /build/build /app/build
COPY docker/nginx.conf /etc/nginx/nginx.conf
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Trois process, donc trois jeux de variables — et **pas** un `PORT` commun :
# le serveur SvelteKit lit `PORT`/`HOST`, l'API Rust aussi. Le superviseur
# exporte les bons noms au lancement de chacun (voir `docker-entrypoint.sh`).
#
# Tout écoute sur la boucle locale du conteneur, sauf nginx : publier un port
# (`-p 5100:8080`) ne relaie pas vers `127.0.0.1`, et seul nginx doit être
# joignable depuis l'extérieur (l'API n'a pas d'authentification).
ENV API_PORT=8091 \
    API_HOST=127.0.0.1 \
    UI_PORT=3100 \
    UI_HOST=127.0.0.1 \
    NODE_ENV=production \
    BOOKS_ROOT=/books \
    EASYBOOK_CONFIG=/app/config.yml \
    EASYBOOK_HOST=127.0.0.1

# `PUBLIC_EASYBOOK_API=/` : l'API est sur la **même origine** que l'interface
# (nginx). Lue au démarrage par le serveur SvelteKit (`$env/dynamic/public`),
# la valeur reste surchargeable pour un déploiement en deux parties.
ENV PUBLIC_EASYBOOK_API=/

# L'API écrit dans la bibliothèque (création de catégories, dépôt de livres) :
# le dossier doit exister et appartenir à l'utilisateur du conteneur, sinon un
# volume nommé serait créé avec les droits de root.
RUN mkdir -p /books && chown node:node /books

USER node
EXPOSE 8080

# Deux requêtes : l'API (par nginx) et l'interface (par nginx aussi). Un
# conteneur dont une seule moitié répond se diagnostique très mal.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/health >/dev/null \
        && curl -fsS http://127.0.0.1:8080/ >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
