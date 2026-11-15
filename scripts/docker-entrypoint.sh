#!/bin/bash
#
# PID 1 du conteneur easybook : nginx, l'interface SvelteKit et l'API Rust
# tournent **ensemble**, dans un seul conteneur.
#
# Pourquoi un superviseur plutôt qu'un `&` dans un shell :
#
#  - `docker stop` n'envoie SIGTERM qu'au PID 1. Sans relais, les trois enfants
#    seraient tués brutalement (SIGKILL) après le délai de grâce — et l'API
#    écrit des fichiers dans la bibliothèque (dépôt de livres) ;
#  - si un seul des trois process meurt, le conteneur doit s'arrêter : un
#    conteneur « vivant » avec une moitié en panne se diagnostique très mal.
#
# Les ports ne sont pas un `PORT` commun : le serveur SvelteKit lit `PORT` et
# `HOST`, l'API Rust lit `PORT` et `EASYBOOK_HOST`. Chaque process reçoit donc
# les siens au lancement (`VAR=valeur commande &`), sans les partager.
set -euo pipefail

api_port="${API_PORT:-8091}"
api_host="${API_HOST:-127.0.0.1}"
ui_port="${UI_PORT:-3100}"
ui_host="${UI_HOST:-127.0.0.1}"

echo "easybook : nginx sur :8080, interface sur :${ui_host}:${ui_port}, API sur :${api_host}:${api_port}"

# nginx au premier plan (`daemon off;`) pour rester un enfant supervisé.
nginx -g 'daemon off;' &
gateway=$!

PORT="$api_port" EASYBOOK_HOST="$api_host" easybook &
api=$!

PORT="$ui_port" HOST="$ui_host" node /app/build/index.js &
ui=$!

shutdown() {
    trap - TERM INT
    kill -TERM "$gateway" "$api" "$ui" 2>/dev/null || true
    wait "$gateway" "$api" "$ui" 2>/dev/null || true
}
trap shutdown TERM INT

# `wait -n` rend la main dès que **l'un** des process s'arrête.
status=0
wait -n || status=$?
echo "easybook : un process s'est arrêté (code $status), arrêt du conteneur"

shutdown
exit "$status"
