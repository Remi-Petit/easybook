#!/usr/bin/env bash
# Lance un scénario k6 en mesurant en parallèle les ressources du conteneur,
# puis produit le rapport.
#
#   ./run.sh smoke
#   ./run.sh load -e PEAK_VUS=100
#   BASE_URL=http://localhost:5100 CONTAINER=easybook ./run.sh stress
#
# Variables : BASE_URL (http://localhost:5200)  CONTAINER (easybook)
#             EB_EMAIL / EB_PASSWORD (compte de dev du compose)  INTERVAL (1 s)
#             COOLDOWN (30 s, mesure après la charge)
#             K6_IMAGE (grafana/k6:latest, utilisé si `k6` n'est pas installé)
# Tout le reste est transmis à `k6 run` (-e CLE=valeur, --vus, …).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

scenario=${1:-}
if [[ -z "$scenario" || ! -f "scenarios/$scenario.js" ]]; then
  echo "usage: ./run.sh <scénario> [options k6]" >&2
  echo "scénarios : $(cd scenarios && ls *.js | sed 's/\.js$//' | tr '\n' ' ')" >&2
  exit 2
fi
shift

export BASE_URL=${BASE_URL:-http://localhost:5200}
export CONTAINER=${CONTAINER:-easybook}
cooldown=${COOLDOWN:-30}

curl -fsS "$BASE_URL/api/health" > /dev/null || { echo "$BASE_URL injoignable" >&2; exit 1; }
docker inspect "$CONTAINER" > /dev/null || { echo "conteneur $CONTAINER introuvable" >&2; exit 1; }

out="results/$(date +%Y%m%d-%H%M%S)-$scenario"
mkdir -p "$out"

{
  echo "date: $(date -Is)"
  echo "scénario: $scenario $*"
  echo "url: $BASE_URL"
  echo "git: $(git rev-parse --short HEAD 2>/dev/null || echo inconnu)"
  echo "hôte: $(nproc) CPU, $(free -m | awk '/^Mem/{print $2}') Mo RAM (k6 tourne sur la même machine sauf si BASE_URL est distant)"
  docker inspect -f 'conteneur: image={{.Config.Image}} limite_cpu_nano={{.HostConfig.NanoCpus}} limite_mem_octets={{.HostConfig.Memory}}' "$CONTAINER"
  echo "bibliothèque: $(curl -fsS "$BASE_URL/api/books" 2>/dev/null | grep -o '"total":[0-9]*' || echo 'protégée par session')"
} > "$out/meta.txt"

./monitor.sh "$out" &
monitor=$!
trap 'kill -TERM "$monitor" 2>/dev/null || true; wait "$monitor" 2>/dev/null || true' EXIT

if command -v k6 > /dev/null; then
  k6=(k6)
else
  image=${K6_IMAGE:-grafana/k6:latest}
  # Chromium n'est que dans l'image `with-browser`.
  [[ "$scenario" == ui ]] && image=${K6_BROWSER_IMAGE:-grafana/k6:master-with-browser}
  k6=(docker run --rm -i --network host --user "$(id -u):$(id -g)" \
    -e HOME=/tmp -e K6_BROWSER_HEADLESS=true -e K6_BROWSER_ARGS=no-sandbox \
    -v "$PWD:/bench" -w /bench "$image")
fi

# Le smoke vérifie les routes : pas de temps de réflexion (surchargeable par -e).
defaults=()
[[ "$scenario" == smoke ]] && defaults=(-e THINK_MIN=0 -e THINK_MAX=0)

# Identifiants de session : passés à k6, jamais écrits dans la ligne de commande
# si l'on s'en tient aux variables d'environnement (le défaut vit dans lib/auth.js).
if [[ -n "${EB_EMAIL:-}" ]]; then defaults+=(-e "EB_EMAIL=$EB_EMAIL"); fi
if [[ -n "${EB_PASSWORD:-}" ]]; then defaults+=(-e "EB_PASSWORD=$EB_PASSWORD"); fi

set +e
"${k6[@]}" run --quiet -e "BASE_URL=$BASE_URL" "${defaults[@]}" \
  --summary-export "$out/summary.json" \
  --out "json=$out/points.json.gz" \
  "$@" "scenarios/$scenario.js"
status=$?
set -e

echo "Fin de charge, mesure de la retombée pendant ${cooldown}s…"
sleep "$cooldown"
kill -TERM "$monitor" 2>/dev/null || true
wait "$monitor" 2>/dev/null || true
trap - EXIT
rm -f "$out/.stop"

if command -v node > /dev/null; then
  node report.mjs "$out" || true
fi

echo "Résultats : bench/k6/$out (code k6 : $status)"
exit "$status"
