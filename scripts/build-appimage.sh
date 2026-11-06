#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)}"
VERSION="${VERSION#v}"
ARCH="x86_64"
APP_NAME="tz-controller-${VERSION}-${ARCH}.AppImage"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
DIST="$ROOT/dist"
TOOLS_DIR="${APPIMAGE_TOOLS_DIR:-$TARGET_DIR/appimage-tools}"
CACHE_DIR="$TARGET_DIR/appimage-cache"
APPIMAGE_DIR="$TARGET_DIR/appimage"
APPDIR="$APPIMAGE_DIR/AppDir"
DESKTOP_ID="io.github.tommyz7.TzController"

LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_SHA="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
APPIMAGETOOL_SHA="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
NOTO_ZIP_URL="${NOTO_ZIP_URL:-https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/06_NotoSansCJKjp.zip}"
NOTO_ZIP_SHA="${NOTO_ZIP_SHA:-bf5b26feb83ee02533d4eb4a127bd90ed9d42bdd7ca7cb6e4030b2f21ef55cc5}"

FONT_ONLY=0
for argument in "$@"; do
    case "$argument" in
        --font-only) FONT_ONLY=1 ;;
        *) printf 'error: unknown argument: %s\n' "$argument" >&2; exit 2 ;;
    esac
done

[[ "$VERSION" =~ ^[0-9A-Za-z._-]+$ ]] || {
    printf 'error: version contains unsupported characters: %s\n' "$VERSION" >&2
    exit 2
}
case "$(uname -m)" in
    x86_64|amd64) ;;
    *) printf 'error: AppImage builds are supported only on x86_64 (found %s)\n' "$(uname -m)" >&2; exit 2 ;;
esac

log() { printf '==> %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"; }

fetch_pinned() {
    local url="$1"
    local path="$2"
    local sha="$3"

    if [ -f "$path" ] && printf '%s  %s\n' "$sha" "$path" | sha256sum -c - >/dev/null 2>&1; then
        chmod +x "$path"
        return 0
    fi
    rm -f "$path" "$path.part"
    log "downloading $(basename "$path")"
    curl -L --fail --retry 3 --connect-timeout 20 --max-time 600 -o "$path.part" "$url"
    printf '%s  %s\n' "$sha" "$path.part" | sha256sum -c - >/dev/null
    mv "$path.part" "$path"
    chmod +x "$path"
}

find_noto_font() {
    local candidate
    if [ "${APPIMAGE_FORCE_FONT_DOWNLOAD:-0}" != "1" ]; then
        for candidate in \
            /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
            /usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc \
            /usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc \
            /usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf \
            /usr/share/fonts/noto-cjk/NotoSansCJKjp-Regular.otf; do
            if [ -f "$candidate" ]; then
                printf '%s\n' "$candidate"
                return 0
            fi
        done
        if command -v fc-match >/dev/null 2>&1; then
            candidate="$(fc-match -f '%{file}\n' 'Noto Sans CJK JP' 2>/dev/null | head -n 1 || true)"
            case "$(basename "$candidate" 2>/dev/null || true)" in
                NotoSansCJK*|NotoSansMonoCJK*)
                    if [ -f "$candidate" ]; then
                        printf '%s\n' "$candidate"
                        return 0
                    fi
                    ;;
            esac
        fi
    fi

    mkdir -p "$CACHE_DIR"
    local zip="$CACHE_DIR/06_NotoSansCJKjp.zip"
    fetch_pinned "$NOTO_ZIP_URL" "$zip" "$NOTO_ZIP_SHA"
    local out="$CACHE_DIR/NotoSansCJKjp-Regular.otf"
    log "extracting Noto Sans CJK JP Regular"
    unzip -p "$zip" NotoSansCJKjp-Regular.otf > "$out"
    [ -s "$out" ] || die "failed to extract NotoSansCJKjp-Regular.otf"
    printf '%s\n' "$out"
}

resolve_lib() {
    local name="$1"
    if command -v ldconfig >/dev/null 2>&1; then
        ldconfig -p 2>/dev/null | awk -v name="$name" '$1 == name {print $NF; exit}'
    fi
}

if [ "$FONT_ONLY" = "1" ]; then
    need curl
    need sha256sum
    need unzip
    find_noto_font
    exit 0
fi

need cargo
need curl
need sha256sum
need patchelf
need mksquashfs
need file
need unzip

mkdir -p "$TOOLS_DIR" "$DIST"
LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL="$TOOLS_DIR/appimagetool-x86_64.AppImage"
fetch_pinned "$LINUXDEPLOY_URL" "$LINUXDEPLOY" "$LINUXDEPLOY_SHA"
fetch_pinned "$APPIMAGETOOL_URL" "$APPIMAGETOOL" "$APPIMAGETOOL_SHA"

FONT="$(find_noto_font)"
log "using font: $FONT"

BUNDLE_LIBS=()
for library in \
    libxkbcommon.so.0 \
    libxkbcommon-x11.so.0 \
    libX11.so.6 \
    libxcb.so.1 \
    libwayland-client.so.0; do
    library_path="$(resolve_lib "$library" || true)"
    if [ -n "$library_path" ] && [ -f "$library_path" ]; then
        BUNDLE_LIBS+=(--library "$library_path")
    else
        log "warning: $library not found; leaving it to the host"
    fi
done

log "building release binary"
cargo build --release --locked

log "staging AppDir"
rm -rf "$APPIMAGE_DIR"
mkdir -p \
    "$APPDIR/usr/bin" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
    "$APPDIR/usr/share/fonts/noto" \
    "$APPDIR/usr/share/doc/tz-controller"

install -m755 "$TARGET_DIR/release/tz-controller" "$APPDIR/usr/bin/tz-controller"
install -m644 "$ROOT/assets/appimage/${DESKTOP_ID}.desktop" "$APPDIR/usr/share/applications/${DESKTOP_ID}.desktop"
install -m644 "$ROOT/assets/appimage/tz-controller.png" "$APPDIR/usr/share/icons/hicolor/512x512/apps/${DESKTOP_ID}.png"
install -m644 "$FONT" "$APPDIR/usr/share/fonts/noto/$(basename "$FONT")"
install -m644 "$ROOT/assets/fonts/OFL.txt" "$APPDIR/usr/share/doc/tz-controller/OFL.txt"
install -m644 "$ROOT/LICENSE" "$APPDIR/usr/share/doc/tz-controller/LICENSE"
install -m755 "$ROOT/assets/appimage/AppRun" "$APPDIR/AppRun"

log "deploying dependencies with linuxdeploy"
export APPIMAGE_EXTRACT_AND_RUN=1
export NO_STRIP=1
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/tz-controller" \
    --desktop-file "$APPDIR/usr/share/applications/${DESKTOP_ID}.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/512x512/apps/${DESKTOP_ID}.png" \
    --custom-apprun "$ROOT/assets/appimage/AppRun" \
    "${BUNDLE_LIBS[@]}" \
    --exclude-library 'libGL.so*' \
    --exclude-library 'libEGL.so*' \
    --exclude-library 'libGLX.so*' \
    --exclude-library 'libdrm.so*' \
    --exclude-library 'libgbm.so*'

# linuxdeploy may replace AppRun; ensure our fontconfig wrapper is in place.
install -m755 "$ROOT/assets/appimage/AppRun" "$APPDIR/AppRun"

log "generating AppImage"
export VERSION
rm -f "$DIST/$APP_NAME"
ARCH="$ARCH" "$APPIMAGETOOL" --no-appstream "$APPDIR" "$DIST/$APP_NAME"

[ -f "$DIST/$APP_NAME" ] || die "AppImage was not generated"
chmod +x "$DIST/$APP_NAME"
log "bundled libraries:"
find "$APPDIR/usr/lib" -maxdepth 1 -type f -printf '  %f\n' 2>/dev/null | sort || true
log "created $DIST/$APP_NAME"
ls -lh "$DIST/$APP_NAME"
