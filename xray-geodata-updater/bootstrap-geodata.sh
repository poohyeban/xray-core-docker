#!/usr/bin/env bash
# First installation only: no controller call and no running-container restart.
set -Eeuo pipefail
umask 027
GEODATA_DIR="${GEODATA_DIR:-/geodata}"
BASE_URL="${BASE_URL:-https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download}"
[[ "$GEODATA_DIR" != / && -d "$GEODATA_DIR" && -w "$GEODATA_DIR" ]]
for file in geoip.dat geosite.dat; do
  if [[ -e "$GEODATA_DIR/$file" ]]; then
    echo 'Geodata already exists; initialization refuses to overwrite it.' >&2
    exit 1
  fi
done
staging="$(mktemp -d "$GEODATA_DIR/.bootstrap.XXXXXX")"
trap 'rm -rf -- "$staging"' EXIT
for file in geoip.dat geosite.dat; do
  curl -fsSL --retry 3 --connect-timeout 15 --max-time 180 \
    "$BASE_URL/$file" -o "$staging/$file"
  curl -fsSL --retry 3 --connect-timeout 15 --max-time 60 \
    "$BASE_URL/$file.sha256sum" -o "$staging/$file.sha256sum"
  # Validate only the expected filename; do not execute checksum-file paths.
  hash="$(awk 'NR == 1 {print $1}' "$staging/$file.sha256sum")"
  [[ "$hash" =~ ^[[:xdigit:]]{64}$ ]]
  printf '%s  %s\n' "$hash" "$staging/$file" | sha256sum -c -
  [[ "$(stat -c %s "$staging/$file")" -ge 1048576 ]]
done
for file in geoip.dat geosite.dat; do
  chmod 0644 "$staging/$file"
  mv -- "$staging/$file" "$GEODATA_DIR/$file"
done
echo 'Initial geodata download and verification completed.'
