# syntax=docker/dockerfile:1
#
# Interface easybook : SvelteKit (adapter-node). Le contexte de construction est
# la **racine** du dépôt (voir docker-compose.yml).

# ── 1. Compilation ─────────────────────────────────────────────────────────
FROM oven/bun:1.3.8-slim AS builder

WORKDIR /build

COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

# ── 2. Exécution ───────────────────────────────────────────────────────────
FROM node:22-bookworm-slim AS runtime

# `PUBLIC_EASYBOOK_API` est lue **au démarrage** (`$env/dynamic/public`) : la
# même image sert une API locale ou distante sans être reconstruite.
ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=3000

WORKDIR /app
# `build/` est autonome (serveur + client compilés, dépendances incluses).
COPY --from=builder /build/build ./build

USER node
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD node -e "fetch('http://127.0.0.1:3000/').then((r) => process.exit(r.ok ? 0 : 1)).catch(() => process.exit(1))"

CMD ["node", "build"]
