"""Cuts the static weights of Inter that the desktop client draws from the Mac app's variable font.

GPUI on Linux draws a variable font at its default weight, so Inter's medium, semibold, bold and
extra bold would all come out regular there. The desktop client therefore embeds one static file
per weight, instanced from `apps/mac/Resources/Fonts/Inter.ttf` (no second source of the design:
the weights are the variable font's own). The files keep the glyphs of Latin, Greek and Cyrillic
text and the symbols the interface uses.

Inter's italic is its own slant axis (`slnt`, -10 degrees), so the italic cuts, regular and bold
(the weights an emphasis in a reply takes), come from the same file at the full slant. They are
flagged as italic faces so that the text system picks them for italic text instead of drawing
the upright face.

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
ITALICS = {"Italic": 400, "BoldItalic": 700}
SLANT = -10
RANGES = (
    "U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,"
    "U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+25A0-25FF,U+2713,U+2715"
)


def flag_italic(font: TTFont, weight: int) -> None:
    """Marks `font` as the italic face of Inter at `weight`, in the tables a text system reads."""
    bold = weight >= 700
    style = "Bold Italic" if bold else "Italic"
    os2 = font["OS/2"]
    os2.fsSelection = (os2.fsSelection & ~0b1000000 & ~0b100000) | 0b1 | (0b100000 if bold else 0)
    font["head"].macStyle = 0b10 | (0b1 if bold else 0)
    postscript = style.replace(" ", "")
    for record in font["name"].names:
        if record.nameID == 2:
            record.string = style
        elif record.nameID == 4:
            record.string = f"Inter {style}"
        elif record.nameID == 6:
            record.string = f"Inter-{postscript}"
        elif record.nameID in (16, 17, 25):
            record.string = {16: "Inter", 17: style, 25: "Inter"}[record.nameID]


def cut(name: str, weight: int, slant: int = 0) -> None:
    font = instancer.instantiateVariableFont(
        TTFont(SOURCE), {"wght": weight, "slnt": slant}, updateFontNames=True
    )
    if slant:
        flag_italic(font, weight)
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
    for name, weight in ITALICS.items():
        cut(name, weight, SLANT)
        print(f"Inter-{name}.ttf")
