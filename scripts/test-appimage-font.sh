#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/font"
printf 'not a real font; only path resolution is tested\n' > "$TMP/font/NotoSansCJKjp-Regular.otf"
python3 - "$TMP/font.zip" "$TMP/font/NotoSansCJKjp-Regular.otf" <<'PY'
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1], "w") as archive:
    archive.write(sys.argv[2], "NotoSansCJKjp-Regular.otf")
PY
SHA="$(sha256sum "$TMP/font.zip" | awk '{print $1}')"

OUT="$(APPIMAGE_FORCE_FONT_DOWNLOAD=1 \
    NOTO_ZIP_URL="file://$TMP/font.zip" \
    NOTO_ZIP_SHA="$SHA" \
    CARGO_TARGET_DIR="$TMP/target" \
    bash "$ROOT/scripts/build-appimage.sh" --font-only)"

COUNT="$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')"
if [ "$COUNT" -ne 1 ]; then
    printf 'error: expected exactly one path line, got %s:\n%s\n' "$COUNT" "$OUT" >&2
    exit 1
fi
if [ ! -f "$OUT" ]; then
    printf 'error: resolved font path does not exist: %s\n' "$OUT" >&2
    exit 1
fi
printf 'font fallback ok: %s\n' "$OUT"
