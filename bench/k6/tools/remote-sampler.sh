#!/bin/sh
# Exécuté DANS le conteneur (via `docker exec -i ... sh -s`) : une ligne CSV par
# processus et par échantillon, lue dans /proc.
#   ts,pid,commande,ticks_cpu,pages_rss,threads,descripteurs_ouverts
# Les descripteurs ne sont comptés que pour l'API (easybook).
# Usage : sh -s <intervalle_s> <fichier_arrêt>

interval=$1
stop=$2

while [ ! -e "$stop" ]; do
  ts=$(date +%s.%N)
  cat /proc/[0-9]*/stat 2>/dev/null | awk -v ts="$ts" '
    {
      comm = $2
      gsub(/[()]/, "", comm)
      fds = 0
      if (comm == "easybook") {
        cmd = "ls /proc/" $1 "/fd 2>/dev/null | wc -l"
        cmd | getline fds
        close(cmd)
      }
      print ts "," $1 "," comm "," $14 + $15 "," $24 "," $20 "," fds
    }'
  sleep "$interval"
done
