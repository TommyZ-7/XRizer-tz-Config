#!/usr/bin/env bash
set -euo pipefail

TAG="${1:?usage: github-release.sh TAG APPIMAGE}"
APP="${2:?usage: github-release.sh TAG APPIMAGE}"

if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$APP" --clobber
else
    gh release create "$TAG" "$APP" \
        --title "XRizer-tz Config ${TAG#v}" \
        --generate-notes
fi
