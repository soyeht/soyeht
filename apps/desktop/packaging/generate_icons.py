#!/usr/bin/env python3
"""Write the desktop client's icons from the Mac app icon set.

The icon is the Mac app's (`apps/mac/Resources/Assets.xcassets/AppIcon.appiconset`, which
`scripts/generate_app_icons.sh` writes from the Soyeht mark); the desktop designs no icon of its
own. This script only repackages those PNGs, at the sizes the set already has, so it needs no
image library:

- `windows/soyeht.ico`: an icon file whose entries are the PNGs themselves (16 to 256 pixels),
  which `build.rs` of `soyeht-desktop` embeds in the executable on Windows.
- `linux/icons/hicolor/<size>x<size>/apps/soyeht.png`: the freedesktop icon theme layout, with
  the name the `com.soyeht.next.desktop` entry gives as its `Icon`.

Run `uv run python apps/desktop/packaging/generate_icons.py` after the Mac set changes and commit
the result. `--check` fails when the committed files differ from what the Mac set produces.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
MAC_SET = ROOT / "apps/mac/Resources/Assets.xcassets/AppIcon.appiconset"
PACKAGING = ROOT / "apps/desktop/packaging"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
# The sizes the Mac set has, as (file, pixels). 1024 is left out: no desktop surface wants it.
SOURCES = {
    16: "icon_16x16.png",
    32: "icon_32x32.png",
    64: "icon_32x32@2x.png",
    128: "icon_128x128.png",
    256: "icon_256x256.png",
    512: "icon_512x512.png",
}
ICO_SIZES = (16, 32, 64, 128, 256)


def png(pixels: int) -> bytes:
    """The Mac set's PNG for a size, checked to be square at that size."""
    data = (MAC_SET / SOURCES[pixels]).read_bytes()
    if not data.startswith(PNG_SIGNATURE):
        raise SystemExit(f"{SOURCES[pixels]} is not a PNG")
    width, height = struct.unpack(">II", data[16:24])
    if (width, height) != (pixels, pixels):
        raise SystemExit(f"{SOURCES[pixels]} is {width}x{height}, expected {pixels}x{pixels}")
    return data


def ico(sizes: tuple[int, ...]) -> bytes:
    """An icon file holding one PNG per size (Windows reads PNG entries since Vista)."""
    images = [png(size) for size in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries = b""
    for size, image in zip(sizes, images):
        # A dimension of 256 is written as 0.
        edge = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", edge, edge, 0, 0, 1, 32, len(image), offset)
        offset += len(image)
    return header + entries + b"".join(images)


def outputs() -> dict[Path, bytes]:
    files = {PACKAGING / "windows/soyeht.ico": ico(ICO_SIZES)}
    for size in SOURCES:
        folder = PACKAGING / f"linux/icons/hicolor/{size}x{size}/apps"
        files[folder / "soyeht.png"] = png(size)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when a file is out of date")
    args = parser.parse_args()
    stale = []
    for path, data in outputs().items():
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                stale.append(path.relative_to(ROOT))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for path in stale:
        print(f"{path} is out of date; run apps/desktop/packaging/generate_icons.py")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
