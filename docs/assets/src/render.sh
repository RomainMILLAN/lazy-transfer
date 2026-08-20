#!/usr/bin/env bash
# Rasterises the HTML sources in this directory to the PNGs in docs/assets/.
#
# The wordmark is rasterised rather than shipped as SVG text on purpose: an SVG
# with a `font-family` renders in whatever font the viewer happens to have, so
# the same file would look different on every machine. `logo-mark.svg` carries
# no text at all and therefore stays vector.
#
# Needs a Chrome/Chromium on PATH and network access for Google Fonts.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$(dirname "$here")"

chrome=""
for candidate in google-chrome chromium chromium-browser; do
  if command -v "$candidate" >/dev/null 2>&1; then
    chrome="$candidate"
    break
  fi
done
if [ -z "$chrome" ]; then
  echo "no chrome/chromium on PATH" >&2
  exit 1
fi

render() {
  local name="$1" width="$2" height="$3"
  "$chrome" --headless --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --virtual-time-budget=6000 \
    --window-size="${width},${height}" \
    --screenshot="${out}/${name}.png" \
    "file://${here}/${name}.html" >/dev/null 2>&1
  echo "rendered ${name}.png"
}

render banner-dark 1280 320
render banner-light 1280 320
render social-preview 1280 640

# Terminal captures. Regenerate their HTML first:
#   cargo run --example screenshots
render screenshot-connection-dark 840 373
render screenshot-connection-light 840 373
render screenshot-browser-dark 1008 522
render screenshot-browser-light 1008 522
