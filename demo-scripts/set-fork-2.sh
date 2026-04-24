#!/usr/bin/env bash

set -euo pipefail

PORT=4003
TARGET=127.0.0.1:3028
LOG_FILE="/tmp/rollback-proxy-${PORT}.log"

echo "Restarting socat proxy on port ${PORT} -> ${TARGET}"

# Kill existing socat listeners on this port
pkill -f "socat TCP-LISTEN:${PORT}" || true

# Small delay to ensure port is freed
sleep 1

# Start new socat proxy
nohup socat TCP-LISTEN:${PORT},reuseaddr,fork TCP:${TARGET} \
  > "${LOG_FILE}" 2>&1 &

PID=$!

echo "Started socat (PID=${PID})"
