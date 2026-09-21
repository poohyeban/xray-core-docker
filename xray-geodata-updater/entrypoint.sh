#!/usr/bin/env bash
set -Eeuo pipefail

CONTROL_SOCKET="${CONTROL_SOCKET:-/run/xray-control/control.sock}"
UPDATER_LOG_DIR="${UPDATER_LOG_DIR:-/var/log/xray-geodata-updater}"

umask 027
mkdir -p "$UPDATER_LOG_DIR"
touch "$UPDATER_LOG_DIR/update.log" "$UPDATER_LOG_DIR/cron.log" "$UPDATER_LOG_DIR/cron-heartbeat"

for _ in $(seq 1 60); do
  if [ -S "$CONTROL_SOCKET" ]; then
    break
  fi
  sleep 1
done

if [ ! -S "$CONTROL_SOCKET" ]; then
  echo "Controller socket not available: $CONTROL_SOCKET" >&2
  exit 1
fi

echo "xray-geodata-updater started"
echo "Timezone: ${TZ:-UTC}"
echo "Geodata schedule: daily at 04:15"
echo "Logrotate schedule: daily check at 03:30"
echo "Controller socket: $CONTROL_SOCKET"

exec crond -f -l 5 -L "$UPDATER_LOG_DIR/cron.log" -c /etc/crontabs
