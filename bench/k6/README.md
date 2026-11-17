# Benchmarks de charge (k6)

Tests de charge d'easybook **à travers nginx** (interface + API + WebSocket), avec mesure en parallèle de la consommation de chaque process du conteneur (`easybook` = API Rust, `node` = interface SvelteKit, `nginx`).

## Prérequis

- Conteneur easybook lancé (`docker compose up -d`, port 5200 par défaut) avec une vraie bibliothèque : les scénarios lisent `/api/books` et tapent sur de vrais livres.
- `docker` ; k6 est lancé via l'image `grafana/k6` s'il n'est pas installé.
- `node` pour le rapport.

## Lancer

```bash
cd bench/k6
./run.sh smoke                       # 1 passage sur chaque route : à lancer d'abord
./run.sh load -e PEAK_VUS=50         # charge nominale par paliers
./run.sh stress -e PEAK_VUS=300      # au-delà du nominal
./run.sh breakpoint                  # débit croissant jusqu'à la rupture
BASE_URL=http://localhost:5100 CONTAINER=easybook ./run.sh load
```

Chaque run crée `results/<date>-<scénario>/` : `report.md` (rapport), `processes.csv` et `container.csv` (ressources, 1 point/s), `points.json.gz` (points k6), `summary.json`, `meta.txt`. Rapport d'un ancien run : `node report.mjs results/<dossier> --bucket=5 --p95=500`.

## Scénarios

| Scénario | Question | Paramètres (`-e`) |
|---|---|---|
| `smoke` | Tout répond-il ? | – |
| `load` | Combien de lecteurs simultanés en usage normal ? Quelle CPU/RAM à chaque palier ? | `PEAK_VUS=50` `STEP_HOLD=2m` `STEP_RAMP=30s` |
| `stress` | Où la latence se dégrade-t-elle, qui sature en premier ? | `PEAK_VUS=300` `STEP_HOLD=90s` |
| `spike` | Comment réagit-il à un pic brutal, et récupère-t-il la mémoire ? | `BASE_VUS=5` `SPIKE_VUS=200` |
| `soak` | Fuite mémoire, threads ou descripteurs qui croissent ? | `SOAK_VUS=20` `SOAK_DURATION=30m` |
| `breakpoint` | Quel débit maximal (req/s) ? Arrêt à p95 > 2 s ou 5 % d'erreurs. | `MAX_RPS=500` `RAMP=10m` `MAX_VUS=2000` |
| `endpoints` | Combien coûte chaque route, isolée ? | `ENDPOINT_RATE=30` `ENDPOINT_SECONDS=60` |
| `websocket` | Coût mémoire d'un onglet ouvert (WebSocket) ? | `PEAK_SOCKETS=1000` `SOCKET_HOLD=60` |

Communs : `THINK_MIN=2` `THINK_MAX=6` (pause entre actions, s), `SOCKETS=1` (WebSocket par utilisateur, `0` pour couper), `COVERS=24` (couvertures par page), `READ_PAGES=8`, `READ_CHAPTERS=3`. Les autres options de k6 passent telles quelles.

Un « utilisateur » (`load`, `stress`, `spike`, `soak`) ouvre l'application (page, fichiers compilés, catalogue, couvertures), puis lit : BD 35 %, EPUB 15 %, PDF 10 %, rien 40 % (selon ce que contient la bibliothèque). Il garde un WebSocket ouvert.

## Lire les résultats

- **Chronologie** : à chaque tranche de temps, VUs, débit, p95, erreurs, CPU/RAM/threads/descripteurs de chaque process. Chercher le palier où le p95 décolle et le process dont la CPU plafonne (`node` mono-thread ? `easybook` au nombre de cœurs ?).
- **Fuites** : sur `soak`, RSS et fds de `easybook` doivent se stabiliser ; la retombée après la charge (`COOLDOWN=30`) montre si la mémoire est rendue.
- **Capacité** : `breakpoint` donne le débit de saturation ; divisé par les req/s d'un utilisateur (`load` : req/s ÷ VUs), il donne le nombre d'utilisateurs simultanés. La section « Capacité » du rapport donne l'estimation directe d'un `load`/`stress`.

## À savoir

- k6 tourne sur la même machine que l'application par défaut : il lui prend de la CPU. Pour des chiffres propres, lancer k6 depuis une autre machine (`BASE_URL=http://<hôte>:5200`, la mesure des ressources reste faite via le `docker` local, donc exécuter `run.sh` là où tourne le conteneur ou exposer le Docker distant).
- Limiter le conteneur (`cpus:` / `mem_limit:` dans le compose) pour mesurer la capacité d'une machine cible ; `meta.txt` consigne les limites.
- Les couvertures sont en cache disque et mémoire : pour mesurer le démarrage à froid, supprimer le cache (`books/.easybook/covers`) et redémarrer le conteneur.
- Les scénarios ne font que lire : aucun livre ni dossier n'est créé.
- Si k6 sature avant l'application (`dropped_iterations`, CPU de k6), les chiffres ne mesurent pas easybook.
