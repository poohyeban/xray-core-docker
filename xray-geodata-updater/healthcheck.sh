#!/bin/sh
set -eu

HEARTBEAT=/var/log/xray-geodata-updater/cron-heartbeat
MAX_AGE=180

kill -0 1
test -S /run/xray-control/control.sock
test -f "$HEARTBEAT"

now="$(date +%s)"
heartbeat_mtime="$(stat -c %Y "$HEARTBEAT")"
age=$((now - heartbeat_mtime))

test "$age" -ge 0
test "$age" -le "$MAX_AGE"
