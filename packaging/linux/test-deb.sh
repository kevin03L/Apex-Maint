#!/bin/sh
# Recette isolée : systemd ne gère que les services du conteneur éphémère.
set -eu
cd "$(dirname "$0")/../.."
docker build -t apex-maint-recette -f packaging/linux/recette.Dockerfile packaging/linux
container=$(docker run -d --privileged --tmpfs /run --tmpfs /tmp -v "$(pwd)/dist:/paquets:ro" apex-maint-recette)
trap 'docker rm -f "$container" >/dev/null' EXIT HUP INT TERM
# Fin du démarrage de systemd avant d'installer (« degraded » est accepté : le conteneur n'a pas
# tous les services d'une machine).
for attempt in $(seq 1 60); do
  state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
  case "$state" in running | degraded) break ;; esac
  sleep 1
done
docker exec "$container" sh -ec '
  apt-get install -y /paquets/*.deb
  apex-maint config init --port 18110
  systemctl start apex-maint
  for attempt in $(seq 1 60); do
    if curl --fail http://127.0.0.1:18110/api/health; then break; fi
    sleep 1
  done
  curl --fail http://127.0.0.1:18110/api/health
  apt-get remove -y apex-maint
  test -f /var/lib/apex-maint/apex-maint.db
  apt-get purge -y apex-maint
  test ! -d /var/lib/apex-maint
'
