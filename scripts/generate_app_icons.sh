#!/bin/bash
# Regenerates the Mac and iOS app icon sets from the Soyeht mark (SoyehtMark in SoyehtClient).
# Run it after changing the mark, the icon canvases or the default theme, and commit the PNGs.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
swift run --package-path apps/shared SoyehtIconGenerator "$ROOT"
