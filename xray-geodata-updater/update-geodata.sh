#!/usr/bin/env bash
set -Eeuo pipefail

GEODATA_DIR="${GEODATA_DIR:-/geodata}"
BASE_URL="${BASE_URL:-https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download}"
CONTROL_SOCKET="${CONTROL_SOCKET:-/run/xray-control/control.sock}"

FILES=("geoip.dat" "geosite.dat")
TS="$(date +%Y%m%d-%H%M%S)"
LOCK_FILE="/tmp/xray-geodata-update.lock"

TMP_DIR=""
NEED_ROLLBACK=0
CHANGED_FILES=()

log() {
  echo "[$(date -Is)] $*"
}

cleanup_tmp() {
  if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
    rm -rf -- "$TMP_DIR"
  fi
  for f in "${FILES[@]}"; do
    rm -f -- "$GEODATA_DIR/$f.new.$TS" 2>/dev/null || true
  done
}

controller_apply() {
  local response_file http_code
  response_file="$(mktemp)"
  http_code="$(
    curl --silent --show-error \
      --unix-socket "$CONTROL_SOCKET" \
      --request POST \
      --max-time 95 \
      --output "$response_file" \
      --write-out '%{http_code}' \
      http://localhost/apply
  )"
  cat "$response_file"
  rm -f -- "$response_file"
  [ "$http_code" = "200" ]
}

rollback() {
  log "Rollback requested."
  for f in "${CHANGED_FILES[@]}"; do
    if [ -f "$GEODATA_DIR/$f.bak.$TS" ]; then
      log "Restoring $f from backup."
      cp -f -- "$GEODATA_DIR/$f.bak.$TS" "$GEODATA_DIR/$f"
      chmod 0644 "$GEODATA_DIR/$f"
    else
      log "WARNING: backup for $f not found; this file cannot be restored."
    fi
  done

  log "Requesting fixed validation/restart after rollback."
  controller_apply || log "ERROR: controller could not restore Xray after rollback."
}

on_error() {
  local exit_code=$?
  trap - ERR
  log "ERROR: update failed with exit code $exit_code at line ${BASH_LINENO[0]}."
  if [ "$NEED_ROLLBACK" = "1" ]; then
    rollback
  fi
  cleanup_tmp
  exit "$exit_code"
}

trap on_error ERR
trap cleanup_tmp EXIT

exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  log "Another geodata update is already running; exiting."
  exit 0
fi

log "Starting Xray geodata update."

if [ "$GEODATA_DIR" = "/" ] || [ -z "$GEODATA_DIR" ]; then
  log "ERROR: unsafe GEODATA_DIR: $GEODATA_DIR"
  exit 1
fi
if [ ! -d "$GEODATA_DIR" ] || [ ! -w "$GEODATA_DIR" ]; then
  log "ERROR: geodata directory is missing or not writable: $GEODATA_DIR"
  exit 1
fi
if [ ! -S "$CONTROL_SOCKET" ]; then
  log "ERROR: controller socket is unavailable: $CONTROL_SOCKET"
  exit 1
fi

log "Cleaning stale temporary files."
find "$GEODATA_DIR" -maxdepth 1 -type d -name '.update.*' -mtime +1 -exec rm -rf -- {} +
find "$GEODATA_DIR" -maxdepth 1 -type f \
  \( -name 'geoip.dat.new.*' -o -name 'geosite.dat.new.*' \) \
  -mtime +1 -delete

available_kb="$(df -Pk "$GEODATA_DIR" | awk 'NR==2 {print $4}')"
if [ "${available_kb:-0}" -lt 512000 ]; then
  log "ERROR: less than 500 MiB is available under $GEODATA_DIR."
  exit 1
fi

TMP_DIR="$(mktemp -d "$GEODATA_DIR/.update.XXXXXX")"
log "Downloading candidates into a staging directory."

for f in "${FILES[@]}"; do
  curl -fL --retry 3 --retry-delay 5 --connect-timeout 15 --max-time 180 \
    -o "$TMP_DIR/$f" "$BASE_URL/$f"
  curl -fL --retry 3 --retry-delay 5 --connect-timeout 15 --max-time 60 \
    -o "$TMP_DIR/$f.sha256sum" "$BASE_URL/$f.sha256sum"
done

cd "$TMP_DIR"
for f in "${FILES[@]}"; do
  log "Verifying $f."
  sha256sum -c "$f.sha256sum"
  size="$(stat -c '%s' "$f")"
  if [ "$size" -lt 1048576 ]; then
    log "ERROR: downloaded $f is suspiciously small: $size bytes."
    exit 1
  fi
  if [ ! -f "$GEODATA_DIR/$f" ] || ! cmp -s "$f" "$GEODATA_DIR/$f"; then
    CHANGED_FILES+=("$f")
  fi
done

if [ "${#CHANGED_FILES[@]}" -eq 0 ]; then
  log "No geodata changes detected; nothing to apply."
  exit 0
fi

log "Changed files: ${CHANGED_FILES[*]}"
for f in "${CHANGED_FILES[@]}"; do
  if [ -f "$GEODATA_DIR/$f" ]; then
    cp -f -- "$GEODATA_DIR/$f" "$GEODATA_DIR/$f.bak.$TS"
  fi
done
NEED_ROLLBACK=1

for f in "${CHANGED_FILES[@]}"; do
  install -m 0644 "$TMP_DIR/$f" "$GEODATA_DIR/$f.new.$TS"
  test -s "$GEODATA_DIR/$f.new.$TS"
  mv -f -- "$GEODATA_DIR/$f.new.$TS" "$GEODATA_DIR/$f"
done

log "Requesting fixed validation and restart from the controller."
controller_apply

NEED_ROLLBACK=0
log "Pruning geodata backups; keeping the seven newest versions per file."
for f in "${FILES[@]}"; do
  find "$GEODATA_DIR" -maxdepth 1 -type f -name "$f.bak.*" \
    -printf '%T@ %p\n' \
    | sort -nr \
    | awk 'NR > 7 {sub(/^[^ ]+ /, ""); print}' \
    | while IFS= read -r old_backup; do
        [ -n "$old_backup" ] && rm -f -- "$old_backup"
      done
done

log "Xray geodata update completed successfully."
