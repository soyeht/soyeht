"""Cuts the static weights of Inter that the desktop client draws from the Mac app's variable font.

GPUI on Linux draws a variable font at its default weight, so Inter's medium, semibold, bold and
extra bold would all come out regular there. The desktop client therefore embeds one static file
per weight, instanced from `apps/mac/Resources/Fonts/Inter.ttf` (no second source of the design:
the weights are the variable font's own). The files keep the glyphs of Latin, Greek and Cyrillic
text and the symbols the interface uses.

    uv run --with fonttools python scripts/instance_inter_fonts.py
"""

from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "apps/mac/Resources/Fonts/Inter.ttf"
OUT = ROOT / "apps/desktop/crates/soyeht-desktop/assets/fonts"
WEIGHTS = {"Medium": 500, "SemiBold": 600, "Bold": 700, "ExtraBold": 800}
RANGES = (
    "U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,"
    "U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+25A0-25FF,U+2713,U+2715"
)


def cut(name: str, weight: int) -> None:
    font = instancer.instantiateVariableFont(
        TTFont(SOURCE), {"wght": weight, "slnt": 0}, updateFontNames=True
    )
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=subset.parse_unicodes(RANGES))
    subsetter.subset(font)
    OUT.mkdir(parents=True, exist_ok=True)
    font.save(OUT / f"Inter-{name}.ttf")


if __name__ == "__main__":
    for name, weight in WEIGHTS.items():
        cut(name, weight)
        print(f"Inter-{name}.ttf")
