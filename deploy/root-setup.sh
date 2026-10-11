#!/usr/bin/env bash
# Run once as root on the server: sudo bash ~/deploy/root-setup.sh
# Installs the web server config, the app's service and the daily backup, and starts them.
# Postgres must already be up (docker compose up -d in ~/infra/postgres) and the app's .env in place.
set -euo pipefail
d=$(dirname "$(readlink -f "$0")")
install -d /etc/caddy/conf.d
install -m 644 "$d/Caddyfile" /etc/caddy/Caddyfile
install -m 644 "$d/fin.caddy" /etc/caddy/conf.d/fin.caddy
caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
install -m 644 "$d/pebblelab.service" /etc/systemd/system/pebblelab.service
install -m 644 "$d/pg-backup.service" /etc/systemd/system/pg-backup.service
install -m 644 "$d/pg-backup.timer" /etc/systemd/system/pg-backup.timer
systemctl daemon-reload
systemctl enable --now docker caddy pebblelab pg-backup.timer
systemctl reload caddy || systemctl restart caddy
systemctl --no-pager --lines=0 status caddy pebblelab | grep -E "Active|Loaded" || true
