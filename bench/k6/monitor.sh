#!/usr/bin/env bash
# Échantillonne les ressources du conteneur easybook jusqu'à SIGTERM/SIGINT.
#
#   processes.csv : CPU, RSS, threads, descripteurs par process
#                   (easybook = API Rust, node = interface SvelteKit, nginx)
#   container.csv : vue cgroup (`docker stats`) : CPU, mémoire, réseau, disque
#
# Usage : monitor.sh <dossier>      (CONTAINER=easybook INTERVAL=1)
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

out=${1:?usage: monitor.sh <dossier>}
container=${CONTAINER:-easybook}
interval=${INTERVAL:-1}
remote_stop=/tmp/easybook-bench.stop
local_stop="$out/.stop"

mkdir -p "$out"
rm -f "$local_stop"
docker exec "$container" rm -f "$remote_stop"

echo "ts,process,cpu_pct,rss_mb,threads,fds,procs" > "$out/processes.csv"
docker exec -i "$container" sh -s "$interval" "$remote_stop" < tools/remote-sampler.sh \
  | awk -f tools/process-stats.awk >> "$out/processes.csv" &

echo "ts,cpu_pct,mem_mb,mem_limit_mb,net_rx_mb,net_tx_mb,blk_read_mb,blk_write_mb,pids" > "$out/container.csv"
(
  while [ ! -e "$local_stop" ]; do
    line=$(docker stats --no-stream \
      --format '{{.CPUPerc}};{{.MemUsage}};{{.NetIO}};{{.BlockIO}};{{.PIDs}}' "$container" 2>/dev/null) \
      && echo "$(date +%s.%N);$line"
  done | awk -f tools/docker-stats.awk >> "$out/container.csv"
) &

stop() {
  touch "$local_stop"
  docker exec "$container" touch "$remote_stop" 2>/dev/null
}
trap stop TERM INT

# Le premier `wait` est interrompu par le signal ; le second attend les échantillonneurs.
wait
wait
