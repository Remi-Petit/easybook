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
│   │   ├── formats/      # **un fichier par format** (voir plus bas)
│   │   ├── archive.rs    # helpers ZIP partagés (EPUB, CBZ)
│   │   ├── rewrite.rs    # réécriture des URL d'un document (HTML, CSS)
│   │   ├── thumbnail.rs  # couvertures : extraction, vignette, cache
│   │   ├── watcher.rs    # interprétation des événements du disque
│   │   └── config.rs     # config.yml
│   ├── tests/            # tests d'intégration (routeur complet, vrais fichiers)
│   └── config.yml
├── frontend/         # SvelteKit — bibliothèque + lecteurs
│   ├── src/routes/(catalog)/+layout.svelte   # coquille : barre latérale + contenu
│   ├── src/routes/(catalog)/+page.svelte     # accueil : tous les livres
│   ├── src/routes/(catalog)/folder/[...path]/ # une catégorie / un dossier
│   ├── src/routes/read/[...path]/       # le lecteur (plein écran)
│   ├── src/lib/catalog.ts               # découpage des listes plates en arborescence
│   ├── src/lib/library.svelte.ts        # état partagé + WebSocket
│   ├── src/lib/components/              # Sidebar, BookCard, FolderCard
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
- **Un dossier de premier niveau est une catégorie** (`books/mangas`,
  `books/électronique`…). La barre latérale les liste sous « Bibliothèque »,
  avec le nombre de livres de chacune ; cliquer ouvre la catégorie
  (`/folder/<catégorie>`), qui montre **tous ses livres**, y compris ceux rangés
  dans ses sous-dossiers, avec un fil d'Ariane.
- La **page d'accueil** montre la totalité des livres, toutes catégories
  confondues (avec la recherche). Le lecteur s'ouvre en plein écran, et son lien
  de retour ramène à la catégorie du livre.

## Formats

| Format | Lecture | Couverture | Comment |
|---|---|---|---|
| `.epub` | ✅ | celle déclarée par le livre (`properties="cover-image"` ou `<meta name="cover">`) | sommaire (`spine` + `toc.ncx` / navigation EPUB 3), chapitres affichés dans l'interface ; images, CSS et polices servis par l'API |
| `.cbz` | ✅ | première page de l'archive | archive ZIP d'images, pages triées naturellement (`p2` avant `p10`) |
| `.cbr` | ✅ | première page de l'archive | archive RAR, lue avec unrar (compilé avec le projet) |
| `.pdf` | ✅ | plus grande image de la première page | le lecteur PDF du navigateur |

Le **conteneur** d'une bande dessinée est détecté par les octets du fichier, pas
par son extension : beaucoup de `.cbr` distribués en ligne sont des ZIP
renommés, et ils se lisent aussi (et inversement).

Les couvertures sont **réduites** (400 px de large, JPEG) et gardées en mémoire par
le backend : le catalogue affiche des dizaines de vignettes sans télécharger les
planches d'origine, qui pèsent plusieurs mégaoctets chacune. Une couverture
absente n'est pas une erreur — l'interface montre alors un repli avec le format.

## Ajouter un format

Chaque format tient dans **un fichier** de `backend/src/formats/` :

```
formats/
├── mod.rs    # Format (extension, type MIME), trait BookFormat, registre module()
├── epub.rs   # chapitres, ressources, couverture
├── cbz.rs    # bande dessinée (délègue à comic.rs)
├── cbr.rs    # bande dessinée RAR (délègue à comic.rs)
├── comic.rs  # archives de planches : ZIP ou RAR, choisi par les octets
└── pdf.rs    # couverture (image de la page 1)
```

1. un fichier qui implémente `BookFormat` (`outline`, `pages`, `cover`,
   `chapter`, `asset`, `page` — tout a une valeur par défaut) ;
2. une variante dans `Format` (extension + type MIME) ;
3. une ligne dans `module()` — et dans `ALL` pour l'annonce au démarrage.

L'API ne connaît que le trait : elle n'a pas à être modifiée.


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
L'image de compilation installe `g++` et `make` : `unrar` (CBR) compile ses
sources C++ avec le projet.

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

## Tests

```bash
cd backend  && cargo test        # unitaires + intégration
cd frontend && bun run test      # logique du catalogue (Vitest)
```

- **Unitaires (backend)** — un module par sujet : tri naturel et détection de
  format, scan du dossier, réécriture HTML/CSS, EPUB (sommaire, chapitres,
  couverture, ressources), CBZ/CBR (pages, ordre, ZIP nommé `.cbr`), PDF
  (extraction de l'image, PDF sans image), vignettes, chemins sûrs.
- **Intégration (backend)** — `tests/api.rs` monte le **vrai routeur** sur une
  bibliothèque temporaire (EPUB + CBZ + PDF fabriqués) et vérifie le contrat
  HTTP : catalogue, fiches, couvertures des quatre formats, chapitres, pages,
  cache, refus de traversée.
- **Frontend** — `catalog.ts` (parent, enfants, aperçu d'un dossier, fil
  d'Ariane, URL).
- **E2E navigateur** : non installés ici — ils demanderaient Chromium + les deux
  process lancés, pour vérifier ce que les tests d'intégration couvrent déjà
  côté API. L'interface a été vérifiée manuellement (catalogue, dossiers,
  lecteurs EPUB/CBZ/CBR/PDF, temps réel).

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
