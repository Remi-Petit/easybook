# easybook

[![CI](https://github.com/Remi-Petit/easybook/actions/workflows/ci.yml/badge.svg)](https://github.com/Remi-Petit/easybook/actions/workflows/ci.yml)
[![sécurité](https://github.com/Remi-Petit/easybook/actions/workflows/security.yml/badge.svg)](https://github.com/Remi-Petit/easybook/actions/workflows/security.yml)
[![license](https://img.shields.io/github/license/Remi-Petit/easybook)](LICENSE)
[![image](https://img.shields.io/badge/image-ghcr.io%2Fremi--petit%2Feasybook-blue)](https://github.com/Remi-Petit/easybook/pkgs/container/easybook)
<!-- couverture:start -->
[![couverture backend](https://img.shields.io/badge/coverage%20backend-90.0%25-brightgreen)](https://github.com/Remi-Petit/easybook/actions/workflows/ci.yml) [![couverture frontend](https://img.shields.io/badge/coverage%20frontend-96.8%25-brightgreen)](https://github.com/Remi-Petit/easybook/actions/workflows/ci.yml)
<!-- couverture:end -->

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
│   ├── src/routes/(catalog)/folder/[...path]/ # une catégorie / une série
│   ├── src/routes/read/[...path]/       # le lecteur (plein écran)
│   ├── src/lib/catalog.ts               # découpage des listes plates en arborescence
│   ├── src/lib/library.svelte.ts        # état partagé + WebSocket
│   ├── src/lib/i18n/                    # langues (un fichier JSON par langue)
│   ├── src/hooks.server.ts              # langue de la requête + attribut `lang`
│   ├── src/lib/components/              # Sidebar, BookCard, FolderCard
│   └── src/lib/reader/                  # EPUB, bande dessinée, PDF
├── docker/
│   ├── nginx.conf          # point d'entrée : /api et /ws vers l'API, le reste vers SvelteKit
│   └── docker-compose.yml  # déploiement (image publiée sur GHCR)
├── Dockerfile          # **une** image : nginx + interface + API
├── docker-compose.release.yml  # image de production construite en local (release)
└── docker-compose.yml  # développement (construit l'image, backend en profil dev)
```

## L'idée

- Le **backend ne fait qu'une chose** : décrire `books/` et servir le contenu des
  livres. Aucune base de données, aucun index à maintenir.
- On peut **aussi ranger ses livres depuis l'interface** : deux icônes à droite du
  titre de la page — « + » ouvre une boîte pour créer un dossier (une catégorie
  à la racine), « ? » explique les dépôts. **Glisser-déposer** un fichier, ou un
  **dossier entier**, n'importe où sur la page : les livres vont dans le dossier
  affiché et l'arborescence du dossier déposé est reproduite. Le serveur écrit
  chaque fichier en flux, dans un temporaire caché renommé une fois complet : un
  envoi interrompu ne laisse jamais un livre tronqué dans le catalogue.
- Les **doublons** sont pris en compte, pas subis : un fichier que le catalogue
  connaît déjà n'est même pas envoyé (le serveur refuse de toute façon en 409),
  un dossier déjà présent reçoit simplement les nouveaux livres, et la fin d'un
  dépôt affiche un bilan — *« 12 ajoutés · 2 déjà présents »* avec les noms des
  doublons — au lieu d'une ligne d'erreur par fichier. Rien n'est jamais écrasé.
- Les messages sont **éphémères** : bilan d'un dépôt, dossiers créés et fichiers
  ignorés s'empilent en bas à droite dans des encarts qui **partent tout seuls**,
  avec une **barre qui se vide** pour montrer le temps restant. La barre *est* la
  minuterie (une animation CSS de `100 %` à `0`) : le message n'est retiré qu'à sa
  fin, et le survol la met en pause pour laisser le temps de lire.
- Le dossier est **surveillé** (`notify`) : un fichier ajouté, renommé ou
  supprimé est poussé aux navigateurs par WebSocket. **Pas de sondage.**
- Un livre est reconnu à son **extension** ; son contenu n'est lu qu'au moment
  où on l'ouvre — sauf le **nombre de pages**, compté une fois au scan puis
  gardé en mémoire (`pages.rs`), parce que chaque carte l'affiche.
- `books/` peut contenir les livres à sa racine **et** dans des sous-dossiers,
  sur n'importe quelle profondeur.
- **Un dossier de premier niveau est une catégorie** (`books/mangas`,
  `books/électronique`…). La barre latérale les liste sous « Bibliothèque »,
  avec le nombre de livres de chacune ; cliquer ouvre la catégorie
  (`/folder/<catégorie>`), avec un fil d'Ariane.
- **Un sous-dossier s'affiche comme une série** : la couverture de son **premier
  livre direct**, son nom, et son nombre de livres — à **tous les niveaux**. Un
  livre rangé dans un sous-dossier ne sert jamais de couverture au niveau
  au-dessus (une histoire bonus mise à part ne devient pas la couverture de la
  série) ; si la série n'a aucun livre à elle, on descend dans sa première
  sous-série, récursivement. La tuile se clique pour l'ouvrir.
- La **page d'accueil** montre la totalité des livres, toutes catégories
  confondues (avec la recherche). Le lecteur s'ouvre en plein écran, et son lien
  de retour ramène au dossier du livre.
- Dans le lecteur (BD et EPUB), la **zone de lecture est découpée en trois** :
  clic à gauche → page précédente, à droite → page suivante, au milieu →
  l'en-tête, le pied de page et le sommaire s'effacent, **marges comprises**, pour
  laisser toute la place au livre (un second clic les ramène, comme **Échap**).
  Les flèches du clavier font la même chose, et un lien du livre reste un lien :
  suivre une note ne tourne pas la page.

## Langues

L'interface parle **français, anglais, allemand, espagnol, italien et
portugais** ; la langue se choisit dans le pied de la barre latérale.

- Le choix est retenu dans le cookie `easybook_locale` : le serveur le relit à
  chaque requête (`hooks.server.ts`), donc la page arrive **déjà traduite** —
  pas de bascule après le chargement.
- Sans choix explicite, la langue du navigateur (`Accept-Language`) est suivie,
  à défaut le français.
- Ajouter une langue = **un fichier** `src/lib/i18n/locales/<code>.json` (copie
  du français, traduite) + une entrée dans `LOCALES`
  (`src/lib/i18n/messages.ts`). Une clé manquante ne compile pas.
- Les pluriels passent par `Intl.PluralRules` : chaque message est soit une
  phrase, soit ses formes (`{ "one": "{count} page", "other": "{count} pages" }`).
  Les nombres et les tailles suivent aussi la langue (`268 Mo` / `268 MB`).

## Formats

| Format | Lecture | Couverture | Comment |
|---|---|---|---|
| `.epub` | ✅ | celle déclarée par le livre (`properties="cover-image"` ou `<meta name="cover">`) | sommaire (`spine` + `toc.ncx` / navigation EPUB 3), chapitres affichés dans l'interface ; images, CSS et polices servis par l'API |
| `.cbz` | ✅ | première page de l'archive | archive ZIP d'images, pages triées naturellement (`p2` avant `p10`) |
| `.cbr` | ✅ | première page de l'archive | archive RAR, lue avec unrar (compilé avec le projet) |
| `.pdf` | ✅ | première image **dessinée** sur la page 1 | lecteur intégré ([pdf.js](https://mozilla.github.io/pdf.js/)) : une page à la fois, texte sélectionnable, zoom, lecture par plages |

Le **conteneur** d'une bande dessinée est détecté par les octets du fichier, pas
par son extension : beaucoup de `.cbr` distribués en ligne sont des ZIP
renommés, et ils se lisent aussi (et inversement).

Chaque carte annonce **le nombre de pages** (`N pages`) sous le livre. Pour un
EPUB, c'est le nombre de **chapitres** : son texte se recompose à l'écran, il
n'a pas de pages fixes. Un livre qui n'a pas pu être ouvert n'annonce rien.

Les couvertures sont **réduites** (400 px de large, JPEG) et gardées en mémoire par
le backend : le catalogue affiche des dizaines de vignettes sans télécharger les
planches d'origine, qui pèsent plusieurs mégaoctets chacune. Une couverture
absente n'est pas une erreur — l'interface montre alors un repli avec le format.

Elles sont aussi **stockées sur disque**, dans `books/.easybook/covers/` (dossier
caché : le scan et le watcher l'ignorent) : un redémarrage ne refait pas le rendu
des PDF. Elles sont **préchauffées** en tâche de fond : au démarrage, puis à chaque
ajout, remplacement ou suppression de livre, le backend produit les vignettes
manquantes (un livre à la fois) et supprime celles des livres disparus, sans
attendre que l'interface les demande. Une vignette est liée au chemin, à la taille
et à la date du livre. Supprimer le dossier est sans risque, il se reconstruit
tout seul. Un `books/` en lecture seule reste utilisable (les couvertures restent
en mémoire seulement).

L'URL d'une couverture porte la date du fichier (une réponse « immuable » côté
navigateur) **et** une révision d'extraction (`COVER_REVISION`) : sans elle, un
changement dans la façon de choisir l'image ne serait jamais réaffiché.

## Ajouter un format

Chaque format tient dans **un fichier** de `backend/src/formats/` :

```
formats/
├── mod.rs    # Format (extension, type MIME), trait BookFormat, registre module()
├── epub.rs   # chapitres, ressources, couverture
├── cbz.rs    # bande dessinée (délègue à comic.rs)
├── cbr.rs    # bande dessinée RAR (délègue à comic.rs)
├── comic.rs  # archives de planches : ZIP ou RAR, choisi par les octets
└── pdf.rs    # couverture (première image dessinée de la page 1)
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

Les comptes passent par un cookie de session : le navigateur doit parler à
l'API **par la même origine** que l'interface. Le serveur de développement
relaie donc `/api` et `/ws` vers `127.0.0.1:8091` ; lancer l'interface avec :

```bash
PUBLIC_EASYBOOK_API=/ bun run dev
```

Au premier démarrage de l'API, l'administrateur créé est annoncé dans la console
(voir [Comptes](#comptes-et-connexion)). Pour travailler sans connexion :
`auth: { enabled: false }` dans `backend/config.yml`.

## Docker

**Un** conteneur, **un** port : nginx sert les fichiers compilés du frontend,
relaie `/api` et `/ws` vers l'API Rust, et le reste vers le serveur SvelteKit.

```bash
docker compose up --build
# interface + API : http://localhost:5200
```

Ce compose est celui du **développement** : le backend est compilé en profil
`dev` (premier passage ~4 min, puis quelques secondes après une modification de
`backend/src/`). Pour tester l'image de **production** (compilation `release`,
longue, comme la CI et la release) :

```bash
docker compose -f docker-compose.release.yml up --build
# interface + API : http://localhost:5300
```

Pour un déploiement, l'image publiée (rien à compiler) :

```bash
cd docker && docker compose up -d
# ou, à la main :
docker run -d --name easybook -p 5100:8080 -v "$PWD/books:/books" \
  ghcr.io/remi-petit/easybook:latest
```

`books/` est monté dans le conteneur (`/books`, via `BOOKS_ROOT`) et doit être
**inscriptible** : l'interface y crée des catégories et y dépose des livres. Sous
Linux, le conteneur tourne sous l'utilisateur `node` (uid 1000) — le dossier
monté doit lui appartenir (`chown -R 1000:1000 books`).

L'image de compilation installe `g++` et `make` : `unrar` (CBR) compile ses
sources C++ avec le projet.

## API

| Route | Rôle |
|---|---|
| `GET /api/health` | état du serveur (public) |
| `/api/auth/…`, `/api/admin/users…` | connexion, comptes, OIDC — voir [Comptes](#comptes-et-connexion) |
| `GET /api/books` | catalogue complet (JSON) |
| `GET /api/book?path=` | fiche d'un livre (titre, chapitres ou nombre de pages) |
| `GET /api/cover?path=&v=` | couverture, en vignette JPEG (`v` = date de modification, pour le cache) |
| `GET /api/admin/performance` | mémoire et CPU du conteneur, process par process (page *Administration → Performances*, administrateurs) |
| `GET /api/chapter?path=&index=` | document d'un chapitre (HTML, URL réécrites) |
| `GET /api/asset?path=&inner=` | ressource interne (image, CSS, police) |
| `GET /api/page?path=&index=` | page d'une bande dessinée |
| `GET /api/raw?path=` | fichier tel quel (PDF, EPUB) |
| `POST /api/folder` | crée une catégorie (`parent: ""`) ou une série |
| `PUT /api/book?folder=&name=` | dépose un livre (le corps **est** le fichier) |
| `GET /ws` | catalogue poussé à chaque changement de `books/` |

Tout `/api` et `/ws` exige une session, sauf `/api/health` et les routes de
connexion.

`path` est toujours **relatif à `books/`** (`Chainsaw Man/T01.cbz`) et rejeté
s'il contient `..`, un chemin absolu ou s'il sort de la racine une fois
canonicalisé.

Chaque livre du catalogue porte son `pages` : le nombre de planches (CBZ/CBR),
les pages du document (PDF), ou les chapitres du `spine` (EPUB). `null` si le
livre n'a pas pu être ouvert.

Le message du WebSocket est exactement le JSON de `/api/books` : à la connexion,
puis à chaque changement.

## Tests

```bash
cd backend  && cargo test        # unitaires + intégration (api, auth, oidc)
cd frontend && bun run test      # catalogue, langues, URL, dépôts, connexion (Vitest)

# contre un vrai Keycloak (docker compose up -d keycloak)
cd backend  && cargo test --test keycloak -- --ignored
```

- **Unitaires (backend)** — un module par sujet : tri naturel et détection de
  format, scan du dossier, réécriture HTML/CSS, EPUB (EPUB 2 `toc.ncx` et EPUB 3,
  sommaire, chapitres, couverture déclarée ou devinée), CBZ/CBR (pages, ordre,
  ZIP nommé `.cbr`), PDF (première image dessinée, ordre des `Do`, formulaires
  imbriqués, filtres `FlateDecode`, pixels bruts, filtres inconnus), vignettes,
  comptage des pages, configuration (fichier, environnement, YAML cassé),
  événements du watcher, chemins sûrs.
- **Intégration (backend)** — `tests/api.rs` monte le **vrai routeur** sur une
  bibliothèque temporaire (EPUB + CBZ + PDF fabriqués) et vérifie le contrat
  HTTP : catalogue (dont le nombre de pages annoncé pour chaque livre), fiches,
  couvertures des quatre formats, chapitres, ressources, pages, cache, fichiers
  bruts, réponses d'erreur (404 / 422), refus de traversée.
- **Comptes (backend)** — `tests/auth.rs` : API fermée sans session, cookie
  `HttpOnly`, essais limités, déconnexion, requêtes inter-sites refusées,
  administration (dernier administrateur protégé, sessions fermées à un
  changement de mot de passe). `tests/oidc.rs` joue le navigateur face à un
  **faux fournisseur** sur un vrai port (PKCE, secret, JWKS) : création, liaison
  par invitation, par identifiant, par e-mail, de son propre compte, et chaque
  faute qu'un jeton peut commettre (audience, émetteur, `nonce`, échéance,
  signature, HS256). `tests/direct.rs` : la connexion directe du mode dev
  (désactivée par défaut, politique de liaison, jetons invalides). `tests/keycloak.rs` refait le parcours contre le **vrai
  Keycloak** du compose.
- **Frontend** — `auth.ts` : garde des pages, redirections sûres, appels de
  l'API, messages d'erreur. `catalog.ts` : parent, enfants, `level()` (les sous-dossiers
  apparaissent comme des séries, à chaque niveau, sans mélange entre
  catégories), `previewBook()` (premier livre **direct**, repli récursif sur la
  première sous-série), fil d'Ariane, URL. `i18n.test.ts` : mêmes clés dans
  toutes les langues, pluriels, remplacement des paramètres, négociation de la
  langue. `api.test.ts` : URL de l'API, tailles et dates dans chaque langue,
  changement de langue.
- **E2E navigateur** : non installés ici — ils demanderaient Chromium + les deux
  process lancés, pour vérifier ce que les tests d'intégration couvrent déjà
  côté API. L'interface a été vérifiée manuellement (catalogue, dossiers,
  lecteurs EPUB/CBZ/CBR/PDF, temps réel).

## Intégration continue

`.github/workflows/ci.yml`, sur `master` et sur les pull requests. Chaque job ne
tourne que si son dossier a changé (`dorny/paths-filter`) : un commit qui ne
touche que le README ne lance rien.

| Job | Ce qu'il vérifie |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --all-targets -- -D warnings` |
| `build_and_test` | `cargo build` + `cargo test`, sur stable, beta et nightly |
| `latest_deps` | la suite avec les dépendances les plus récentes (informatif) |
| `msrv` | `rust-version` de `Cargo.toml` tient (cargo-hack) |
| `audit` | `cargo audit` (avis RustSec) — bloquant |
| `frontend` | `bun run test`, `bun run check`, `bun run build` |
| `couverture` | calcule les taux et réécrit les badges ci-dessus (uniquement sur `master`) |
| `trivy` | l'**image unique** construite par `docker compose` : CRITICAL bloquant, HIGH en rapport |

L'image est construite sans être publiée, avec un cache GHA : la CI vérifie ce
que l'utilisateur lance vraiment, pas seulement les lockfiles.

### Sécurité

`.github/workflows/security.yml` regarde **tout le dépôt** (pas de filtre `paths`) :
à chaque push sur `master`, chaque pull request, et chaque lundi.

| Job | Ce qu'il vérifie |
|---|---|
| `gitleaks` | secrets (clés, jetons, mots de passe) dans le code **et l'historique** ; exceptions documentées dans `.gitleaks.toml` (clés RSA jetables des tests) |
| `semgrep` | motifs de code dangereux (`p/security-audit`, `p/secrets`, `p/typescript`, `p/dockerfile`) — bloquant |

Les deux tournent depuis leur image officielle, version épinglée. `p/rust` n'est
pas activé : il signale chaque `unsafe`, que ce code justifie (`mmap`, `libc`).
En local :

```bash
docker run --rm -v "$PWD:/repo" zricethezav/gitleaks:v8.30.1 detect --source /repo --config /repo/.gitleaks.toml --redact
docker run --rm -v "$PWD:/src" semgrep/semgrep:1.186.0 semgrep scan --config p/security-audit --config p/secrets --metrics=off
```

**OWASP ZAP** (développement) : le `docker-compose.yml` embarque un service `zap`
qui ne démarre jamais seul. Il se connecte en administrateur, explore l'interface
et l'API en **passif** (il observe, il n'attaque pas) et écrit un rapport :

```bash
docker compose --env-file .env.dev run --rm zap
# → zap-reports/zap-report.html (et .json, ignorés par git)
```

Les avertissements restants sont connus : pas de `default-src` / `script-src`
(le rendu des EPUB et pdf.js en demanderaient une très large), et « pas de jeton
anti-CSRF » — la protection est le cookie `SameSite=Lax` plus le contrôle de
l'en-tête `Origin` côté API. Ne scanner que ses propres serveurs.

Les badges de couverture ne sont **jamais** saisis à la main : le job les
réécrit, et ne commite que s'ils ont bougé (`README.md` n'est pas dans les
`paths` du workflow, donc ce commit ne relance pas la CI). Même calcul en local :

```bash
cd backend  && mkdir -p target/cov && cargo llvm-cov --json --summary-only \
                --output-path target/cov/backend-coverage.json
cd frontend && bun run test:coverage
node scripts/coverage.mjs          # réécrit les badges
node scripts/coverage.mjs --check  # exit 1 s'ils sont périmés
```

Périmètre : les lignes que les tests **unitaires** peuvent atteindre.

- **Frontend** : les modules de logique (`catalog.ts`, `api.ts`,
  `i18n/messages.ts`, `i18n/state.svelte.ts`). Les composants et les pages
  `.svelte` sont vérifiés dans le navigateur ; les compter afficherait 0 % pour
  du code qui fonctionne (voir `frontend/vitest.config.ts`).
- **Backend** : tout le crate, `main.rs` compris. Son `main()` (bannière,
  démarrage du serveur, thread du watcher) n'est atteignable par aucun test :
  c'est du câblage, et tout ce qu'il appelle est testé. Les boucles WebSocket de
  `api.rs` et le lecteur RAR de `comic.rs` demandent un client ou une archive
  qu'on ne construit pas en test — ce sont les principaux restes.

## Configuration

`backend/config.yml` :

```yaml
books_root: ../books

limits:
  max_concurrent_reads: 64   # lectures de livre simultanées (les autres attendent)
  max_blocking_threads: 128  # threads bloquants du runtime Tokio
  open_archives: 16          # archives ZIP gardées ouvertes (0 : aucune)
```

L'image Docker embarque ce fichier (`/app/config.yml`) ; pour changer un plafond,
monter le vôtre dessus (`-v ./config.yml:/app/config.yml:ro`). Les planches et
couvertures sont aussi gardées par nginx (`docker/nginx.conf`, 64 Mo de cache).

Variables d'environnement :

| Variable | Défaut | Rôle |
|---|---|---|
| `BOOKS_ROOT` | `books_root` du YAML | dossier des livres (gagne sur le fichier) |
| `EASYBOOK_CONFIG` | `config.yml` | autre fichier de configuration |
| `EASYBOOK_CACHE_DIR` | `<books>/.easybook/covers` | dossier du cache disque des couvertures |
| `EASYBOOK_HOST` | `127.0.0.1` | interface d'écoute de l'API (`0.0.0.0` en conteneur) |
| `PORT` | `8091` | port de l'API |
| `PUBLIC_EASYBOOK_API` (frontend) | `http://<hôte>:8091` | adresse de l'API vue par le navigateur — `/` quand c'est le **même serveur** qui sert les deux (cas de l'image Docker) |
| `EASYBOOK_MODE` | `prod` | `dev` : connexion directe à Keycloak depuis le formulaire (développement uniquement) |
| `EASYBOOK_DATA_DIR` | `<books>/.easybook` | dossier de la base des comptes (`accounts.db`) — **à monter en volume** (`/data` dans l'image) |
| `EASYBOOK_ADMIN_EMAIL`, `EASYBOOK_ADMIN_PASSWORD` | `admin@easybook.local`, aléatoire | administrateur créé au premier démarrage |

## Comptes et connexion

Toute l'application exige une connexion. Les comptes vivent dans une base
**SQLite** (`accounts.db`, dans `EASYBOOK_DATA_DIR`) : identifiants en **UUID**,
connexion par **adresse e-mail** (unique) et mot de passe — jamais par nom —,
mots de passe en Argon2id, sessions par cookie `HttpOnly` / `SameSite=Lax` (dont
seule l'empreinte est stockée).

**Premier démarrage** : sans aucun compte, un administrateur est créé
(`EASYBOOK_ADMIN_EMAIL`, `admin@easybook.local` par défaut). Sans `EASYBOOK_ADMIN_PASSWORD`, son
mot de passe est tiré au hasard et affiché **une seule fois** dans les journaux
(`docker logs easybook`).

La page *Administration → Comptes* crée, désactive, supprime et rétrograde des
comptes, réinitialise un mot de passe (ce qui ferme les sessions du compte) et
gère la liaison OIDC. Le dernier administrateur actif ne peut être ni supprimé
ni rétrogradé. Quelques essais ratés bloquent la connexion pour quelques
minutes.

### OpenID Connect

N'importe quel fournisseur OIDC (Keycloak, Authentik, Google…) : flux
*authorization code* avec **PKCE**, jeton d'identité vérifié (signature,
émetteur, audience, expiration, `nonce`). Le bouton apparaît dès que `issuer` et
`client_id` sont renseignés — dans `config.yml` ou par variables
`EASYBOOK_OIDC_*` (les variables gagnent) :

```yaml
auth:
  enabled: true          # false : plus de connexion, API ouverte (déconseillé)
  session_days: 30
  oidc:
    issuer: https://sso.example.org/realms/maison   # = `iss` des jetons
    discovery_url: ""    # où lire /.well-known/openid-configuration (conteneurs)
    client_id: easybook
    client_secret: ""    # préférer EASYBOOK_OIDC_CLIENT_SECRET
    redirect_url: ""     # vide : déduite de l'adresse du navigateur
    label: Single sign-on
    auto_create: false   # un compte OIDC inconnu crée un compte local
    link_by_email: false # …ou se lie au compte local de même e-mail vérifié
    self_link: false     # chacun peut lier son propre compte
```

À déclarer chez le fournisseur : l'URL de retour
`https://<votre-hôte>/api/auth/oidc/callback`.

**Lier un compte local à un compte OIDC.** Par défaut, un compte OIDC inconnu
est **refusé** : c'est l'administrateur qui décide. Il a trois façons de lier :

1. **lien d'invitation** (page *Comptes* → *Créer un lien d'invitation*) : à
   remettre à la personne, qui s'authentifie chez le fournisseur et arrive
   connectée **à son compte local** — le lien vaut 15 minutes, une seule fois ;
2. **par identifiant** (`sub` du fournisseur), s'il le connaît ;
3. **en laissant faire** : `link_by_email` (e-mail certifié par le fournisseur,
   et un seul compte local porteur de cette adresse), `self_link` (chacun lie
   son compte depuis *Mon compte*) ou `auto_create`.

Un compte lié se connecte des deux façons ; on ne peut pas délier la seule
façon de se connecter d'un compte.

### Production par défaut, développement avec `.env.dev`

Le `docker-compose.yml` est en **production** tant qu'on ne lui dit pas
autrement : pas de Keycloak, pas de connexion directe, OIDC désactivé tant que
`EASYBOOK_OIDC_*` est vide, administrateur au mot de passe aléatoire. Tout se
règle par variables d'environnement ; le développement tient dans `.env.dev` :

```bash
docker compose up -d --build                         # production
docker compose --env-file .env.dev up -d --build     # développement
```

`.env.dev` lance un Keycloak **réservé aux tests** (profil `dev`, console sur
<http://localhost:8180>, `admin` / `admin`), avec le realm `easybook` importé
(`docker/keycloak/easybook-realm.json`) et ses utilisateurs `alice@example.org`,
`bob@example.org` (e-mail vérifié) et `carol@example.org` (non vérifié) — mot de
passe = prénom en minuscules. L'administrateur est `admin@easybook.local` /
`admin-dev-1234`.

**Connexion rapide** : en développement, la page de connexion affiche un bouton
par compte de `EASYBOOK_DEV_LOGINS` (`admin@easybook.local`, `alice@…`, `bob@…`,
`carol@…`) — un clic suffit. Les mots de passe restent dans le serveur ; sans
`EASYBOOK_MODE=dev`, la liste est ignorée et la route n'existe pas.

**Connexion « directe »** (`EASYBOOK_MODE=dev` seulement) : le formulaire de
connexion accepte aussi l'e-mail et le mot de passe d'un compte Keycloak, sans
redirection (*direct access grant* : easybook les transmet à Keycloak et vérifie
le jeton). La même politique de liaison s'applique — compte déjà lié,
`link_by_email`, `auto_create` (activé dans `.env.dev`), sinon refus. En
production le mode est ignoré : ce flux fait passer le mot de passe par
easybook, ce que la redirection évite.

```bash
cd backend && cargo test --test keycloak -- --ignored   # contre le vrai Keycloak
```

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
