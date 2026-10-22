#!/bin/sh
# Construction Debian 12 isolée ; exécuter avec Docker démarré et un accès réseau.
# Usage : sh packaging/linux/build-deb-docker.sh [amd64|arm64]
# Sous Windows, utiliser Git Bash (Docker Desktop en mode conteneurs Linux).
set -eu
cd "$(dirname "$0")/../.."
arch=${1:-amd64}
case "$arch" in
    amd64|arm64) ;;
    *) echo "Architecture non prise en charge : $arch" >&2; exit 1 ;;
esac
if [ "$#" -gt 1 ]; then
    echo "Usage : sh packaging/linux/build-deb-docker.sh [amd64|arm64]" >&2
    exit 1
fi
docker info >/dev/null
mkdir -p dist
depot=$(pwd -P)
# Git Bash doit transmettre des chemins Windows sans convertir ceux du conteneur.
case "$(uname -s)" in
    MINGW*|MSYS*) depot=$(pwd -W); export MSYS_NO_PATHCONV=1 ;;
esac
# Volumes réutilisables : aucun mélange avec target/ ou le registre Cargo Windows.
# L'image officielle fixe Rust >= 1.95 et utilise Debian 12 (bookworm).
docker run --rm --platform "linux/$arch" \
    --mount "type=bind,source=$depot,target=/workspace" \
    --mount "type=volume,source=apex-maint-deb-$arch-target,target=/workspace/target" \
    --mount "type=volume,source=apex-maint-deb-$arch-registry,target=/usr/local/cargo/registry" \
    --mount "type=volume,source=apex-maint-deb-$arch-git,target=/usr/local/cargo/git" \
    --workdir /workspace rust:1.95.0-bookworm sh -ec '
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get install -y --no-install-recommends \
            build-essential pkg-config cmake ca-certificates dpkg-dev \
            libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev libvulkan-dev
        rustc --version
        cargo fetch --locked
        sh packaging/linux/build-deb.sh
        version=$(sed -n '\''s/^version = "\([^"]*\)"/\1/p'\'' Cargo.toml | head -1)
        arch=$(dpkg --print-architecture)
        paquet="dist/apex-maint_${version}_${arch}.deb"
        dpkg-deb -I "$paquet"
        dpkg-deb -c "$paquet"
        sha256sum -c "$paquet.sha256"
        stat -c "%n : %s octets" "$paquet"
    '
