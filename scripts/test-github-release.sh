#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
cat > "$TMP/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'gh %s\n' "$*" >> "$GH_LOG"
case "${1:-} ${2:-}" in
    "release view") [ "${GH_RELEASE_EXISTS:-0}" = "1" ] && exit 0 || exit 1 ;;
    *) exit 0 ;;
esac
EOF
chmod +x "$TMP/bin/gh"

APP="$TMP/app.AppImage"
: > "$APP"
export PATH="$TMP/bin:$PATH"
export GH_LOG="$TMP/log"

export GH_RELEASE_EXISTS=0
: > "$GH_LOG"
bash "$ROOT/scripts/github-release.sh" v0.1.1 "$APP"
grep -q 'gh release create v0.1.1' "$GH_LOG"
grep -q -- '--title tz-controller' "$GH_LOG"

export GH_RELEASE_EXISTS=1
: > "$GH_LOG"
bash "$ROOT/scripts/github-release.sh" v0.1.1 "$APP"
grep -q 'gh release upload v0.1.1' "$GH_LOG"
grep -q -- '--clobber' "$GH_LOG"

printf 'release helper ok\n'
