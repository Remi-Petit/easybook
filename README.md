# easybook

Bibliothèque de livres locale : un dossier `books/`, un backend Rust qui le
surveille, une interface SvelteKit pour lire ce qu'il contient.

```
easybook/
├── books/            # tes livres (EPUB, CBZ, CBR, PDF), à plat ou en dossiers
├── backend/          # Rust — scan, lecture des formats, API HTTP, WebSocket
│   ├── src/
│   │   ├── main.rs       # démarrage + watcher
│   │   ├── api.rs        # routes HTTP et WebSocket
│   │   ├── library.rs    # scan récursif de books/
│   │   ├── formats.rs    # formats reconnus, tri « naturel »
│   │   ├── epub.rs       # sommaire et chapitres d'un EPUB
│   │   ├── archive.rs    # ouverture des archives ZIP (EPUB, CBZ)
│   │   ├── rewrite.rs    # réécriture des URL d'un document (HTML, CSS)
│   │   ├── thumbnail.rs  # couvertures : extraction, vignette, cache
│   │   ├── watcher.rs    # interprétation des événements du disque
│   │   └── config.rs     # config.yml
│   └── config.yml
├── frontend/         # SvelteKit — bibliothèque + lecteurs
│   ├── src/routes/+page.svelte          # le catalogue
│   ├── src/routes/read/[...path]/       # le lecteur
│   ├── src/lib/library.svelte.ts        # état partagé + WebSocket
│   ├── src/lib/components/BookCard.svelte # une vignette du catalogue
│   └── src/lib/reader/                  # EPUB, bande dessinée, PDF
└── docker-compose.yml
```

## L'idée

- Le **backend ne fait qu'une chose** : décrire `books/` et servir le contenu des
  livres. Aucune base de données, aucun index à maintenir.
- Le dossier est **surveillé** (`notify`) : un fichier ajouté, renommé ou
  supprimé est poussé aux navigateurs par WebSocket. **Pas de sondage.**
- Un livre est reconnu à son **extension** ; son contenu n'est lu qu'au moment
  où on l'ouvre.
- `books/` peut contenir les livres à sa racine **et** dans des sous-dossiers,
  sur n'importe quelle profondeur.

## Formats

| Format | Lecture | Couverture | Comment |
|---|---|---|---|
| `.epub` | ✅ | celle déclarée par le livre (`properties="cover-image"` ou `<meta name="cover">`) | sommaire (`spine` + `toc.ncx` / navigation EPUB 3), chapitres affichés dans l'interface ; images, CSS et polices servis par l'API |
| `.cbz` | ✅ | première page de l'archive | archive ZIP d'images, pages triées naturellement (`p2` avant `p10`) |
| `.pdf` | ✅ | repli (pas de rendu de page) | le lecteur PDF du navigateur |
| `.cbr` | ❌ | repli | archive RAR : listé (il est bien là), mais il faut un décodeur RAR pour l'ouvrir |

Les couvertures sont **réduites** (400 px de large, JPEG) et gardées en mémoire par
le backend : le catalogue affiche des dizaines de vignettes sans télécharger les
planches d'origine, qui pèsent plusieurs mégaoctets chacune. Une couverture
absente n'est pas une erreur — l'interface montre alors un repli avec le format.

Ajouter un format = une variante dans `formats.rs` (extension, type MIME) plus,
si le contenu n'est pas déjà servi par l'archive, un module de lecture dédié.

## Lancer en développement

Deux process :

```bash
# terminal 1 — API (http://127.0.0.1:8091)
cd backend
cargo run

# terminal 2 — interface (http://127.0.0.1:5100)
cd frontend
bun install
bun run dev
```

L'interface vise `http://<hôte>:8091` par défaut. Pour une autre adresse :

```bash
PUBLIC_EASYBOOK_API=http://192.168.1.10:8091 bun run dev
```

## Docker

```bash
docker compose up --build
# interface : http://localhost:5100
# API       : http://localhost:8091/api/books
```

`books/` est monté dans le conteneur de l'API (`/books`, via `BOOKS_ROOT`).

## API

| Route | Rôle |
|---|---|
| `GET /api/health` | état du serveur |
| `GET /api/books` | catalogue complet (JSON) |
| `GET /api/book?path=` | fiche d'un livre (titre, chapitres ou nombre de pages) |
| `GET /api/cover?path=&v=` | couverture, en vignette JPEG (`v` = date de modification, pour le cache) |
| `GET /api/chapter?path=&index=` | document d'un chapitre (HTML, URL réécrites) |
| `GET /api/asset?path=&inner=` | ressource interne (image, CSS, police) |
| `GET /api/page?path=&index=` | page d'une bande dessinée |
| `GET /api/raw?path=` | fichier tel quel (PDF, EPUB) |
| `GET /ws` | catalogue poussé à chaque changement de `books/` |

`path` est toujours **relatif à `books/`** (`Chainsaw Man/T01.cbz`) et rejeté
s'il contient `..`, un chemin absolu ou s'il sort de la racine une fois
canonicalisé.

Le message du WebSocket est exactement le JSON de `/api/books` : à la connexion,
puis à chaque changement.

## Configuration

`backend/config.yml` :

```yaml
books_root: ../books
```

Variables d'environnement :

| Variable | Défaut | Rôle |
|---|---|---|
| `BOOKS_ROOT` | `books_root` du YAML | dossier des livres (gagne sur le fichier) |
| `EASYBOOK_CONFIG` | `config.yml` | autre fichier de configuration |
| `EASYBOOK_HOST` | `127.0.0.1` | interface d'écoute de l'API (`0.0.0.0` en conteneur) |
| `PORT` | `8091` | port de l'API |
| `PUBLIC_EASYBOOK_API` (frontend) | `http://<hôte>:8091` | adresse de l'API vue par le navigateur |

⚠️ L'API n'a **aucune authentification** : elle écoute sur la boucle locale par
défaut. Publier un port, c'est décider d'exposer la bibliothèque.

## Notes

- Sous Docker Desktop (Windows/macOS), les événements du système de fichiers ne
  traversent pas toujours le montage : un livre déposé depuis l'hôte peut
  n'apparaître qu'au prochain scan (une requête à `/api/books` en déclenche un).
- La reprise de lecture (chapitre ou page) est mémorisée dans le navigateur
  (`localStorage`), pas côté serveur.
- Le contenu d'un livre est inséré dans la page du lecteur : les gestionnaires
  d'événements en ligne (`onerror=`, `onclick=`…) sont retirés et les scripts
  restent inertes (`innerHTML`), mais un livre reste un fichier **tiers** — ne
  déposez pas dans `books/` des fichiers dont vous ne connaissez pas la
  provenance, surtout si l'API est publiée au-delà de la machine.
- `/api/raw` et une page de bande dessinée sont lus **en mémoire** le temps de
  la réponse : c'est simple et instantané ici, à garder en tête pour de très
  gros fichiers consultés en parallèle.
