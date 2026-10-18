#!/bin/sh
# Construction native amd64 ou arm64, sans téléchargement de dépendance Cargo.
set -eu
cd "$(dirname "$0")/../.."
arch=$(dpkg --print-architecture)
case "$arch" in amd64|arm64) ;; *) echo "Architecture non prise en charge : $arch" >&2; exit 1;; esac
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
cargo build --release --offline --locked -p apex-server -p apex-maint-admin
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
install -d "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/apex-maint/static" "$root/usr/share/applications" "$root/usr/share/polkit-1/rules.d" "$root/lib/systemd/system" "$root/usr/share/doc/apex-maint" "$root/etc/apex-maint"
install -m755 target/release/apex-maint target/release/apex-maint-admin "$root/usr/bin/"
cp -R static/. "$root/usr/share/apex-maint/static/"
install -m644 packaging/linux/systemd/apex-maint.service "$root/lib/systemd/system/"
install -m644 packaging/linux/polkit/50-apex-maint.rules "$root/usr/share/polkit-1/rules.d/"
install -m644 packaging/linux/apex-maint-admin.desktop "$root/usr/share/applications/"
install -m644 README.md docs/INSTALLATION.md docs/ADMINISTRATION.md "$root/usr/share/doc/apex-maint/"
for script in postinst prerm postrm; do install -m755 "packaging/linux/debian/$script" "$root/DEBIAN/$script"; done
cat > "$root/DEBIAN/control" <<EOF
Package: apex-maint
Version: $version
Architecture: $arch
Maintainer: kevin03L
Depends: libc6, libgcc-s1, libstdc++6, libx11-6, libxcb1, libxkbcommon0, libwayland-client0, libvulkan1, adduser
Section: utils
Priority: optional
Description: GMAO multi-sites Apex Maint
 Moteur, interface web et utilitaire d'administration.
EOF
mkdir -p dist
dpkg-deb --root-owner-group --build "$root" "dist/apex-maint_${version}_${arch}.deb"
sha256sum "dist/apex-maint_${version}_${arch}.deb" > "dist/apex-maint_${version}_${arch}.deb.sha256"
