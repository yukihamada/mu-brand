#!/bin/sh
# /data is a volume mounted root-owned on first attach. Make it writable by the
# unprivileged `app` user, then drop privileges and exec the server as non-root.
set -e
mkdir -p /data
chown -R app:app /data 2>/dev/null || true
exec gosu app "$@"
