#!/usr/bin/env bash
# Usage: sudo bash publish.sh /absolute/path/site.tar.gz
# Archive contents: dist/ and deploy/. Never package application source or secrets.
set -euo pipefail
umask 022
archive="${1:?Pass the website release archive}"
[[ "$EUID" -eq 0 && -f "$archive" && ! -L "$archive" ]] || exit 1
base=/var/www/tktiku-site
release="$base/releases/$(date -u +%Y%m%dT%H%M%SZ)"
[[ ! -e "$release" ]] || exit 1
install -d -m 755 "$base/releases" "$release"
python3 - "$archive" "$release" <<'PY'
import pathlib, sys, tarfile
archive, release = sys.argv[1:]
with tarfile.open(archive, 'r:gz') as source:
    members = source.getmembers()
    for member in members:
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] not in ('dist', 'deploy'):
            raise SystemExit('Unexpected archive path')
        if not (member.isfile() or member.isdir()) or member.size > 5_000_000:
            raise SystemExit('Unexpected archive entry')
    if sum(member.size for member in members) > 10_000_000:
        raise SystemExit('Archive too large')
    source.extractall(release, filter='data')
PY
test -s "$release/dist/index.html"
test -s "$release/deploy/nginx.conf"
find "$release" -type d -exec chmod 755 {} +
find "$release" -type f -exec chmod 644 {} +
previous=$(readlink "$base/current" || true)
[[ ! -e "$base/current" || -L "$base/current" ]] || { echo 'Current is not a managed symlink'; exit 1; }
config=/etc/nginx/sites-available/tktiku.cn
stamp=$(date -u +%Y%m%dT%H%M%SZ)
if [[ -f "$config" ]]; then cp -a "$config" "$config.before-$stamp"; fi
rollback() {
    if [[ -f "$config.before-$stamp" ]]; then cp -a "$config.before-$stamp" "$config"; fi
    if [[ -n "$previous" ]]; then
        ln -s "$previous" "$base/rollback-$stamp"
        mv -Tf "$base/rollback-$stamp" "$base/current"
    fi
    nginx -t && systemctl reload nginx
}
trap 'rollback' ERR
ln -s "$release/dist" "$base/next-$stamp"
mv -Tf "$base/next-$stamp" "$base/current"
install -m 644 "$release/deploy/nginx.conf" "$config"
if [[ ! -e /etc/nginx/sites-enabled/tktiku.cn ]]; then
    ln -s "$config" /etc/nginx/sites-enabled/tktiku.cn
fi
nginx -t
systemctl reload nginx
# Reload returns before new workers necessarily start accepting TLS connections.
# Retry read-only checks briefly while retaining full certificate validation.
curl --retry 5 --retry-delay 1 --retry-all-errors --connect-timeout 5 --max-time 15 --noproxy '*' --resolve tktiku.cn:443:127.0.0.1 -fsS https://tktiku.cn/ -o "$release/served-index.html"
cmp "$release/dist/index.html" "$release/served-index.html"
curl --noproxy '*' --resolve tktiku.cn:443:127.0.0.1 -fsS https://tktiku.cn/updates/latest.json -o "$release/served-latest.json"
curl --noproxy '*' -fsS http://127.0.0.1:8080/health/ready
trap - ERR
printf '\nPublished website: %s\nPrevious release: %s\n' "$release" "$previous"
sha256sum "$release/dist/index.html"
