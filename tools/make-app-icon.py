#!/usr/bin/env python3
"""Turn the brand mark into the square source `tauri icon` needs.

    python tools/make-app-icon.py            # → logo/app-icon-1024.png
    node ui/node_modules/@tauri-apps/cli/tauri.js icon logo/app-icon-1024.png \\
        -o crates/mio-app/icons

`logo/mio-sound-manager.png` is 1000x830 with a transparent background. `tauri
icon` refuses a non-square source unless told how to fit it, and `--fit contain`
would letterbox the mark edge-to-edge, which makes the taskbar icon look larger
than every other icon next to it. So the mark is scaled to 86% of the canvas and
centred: that is roughly the padding Windows' own icons carry, and it keeps the
two lobes of the M off the icon border at 16px.

Pillow is the only dependency (`pip install pillow`).

`logo/` is gitignored: this script is the only reader of the source mark, and the
outputs it feeds (`crates/mio-app/icons/`, `ui/src/assets/brand.svg`) are
committed. On a fresh clone the source mark has to be put back by hand first.
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "logo" / "mio-sound-manager.png"
OUTPUT = ROOT / "logo" / "app-icon-1024.png"

SIDE = 1024
# Fraction of the canvas the mark occupies. Windows' first-party icons sit around
# 80–88%; below that the mark reads as a stamp, above it the lobes touch the edge.
FILL = 0.86


def main() -> None:
    mark = Image.open(SOURCE).convert("RGBA")
    # Crop to what is actually drawn: a transparent margin in the source would
    # otherwise count as part of the mark and shrink it a second time.
    mark = mark.crop(mark.getbbox())

    inner = round(SIDE * FILL)
    scale = inner / max(mark.size)
    drawn = mark.resize(
        (max(1, round(mark.width * scale)), max(1, round(mark.height * scale))),
        Image.LANCZOS,
    )

    canvas = Image.new("RGBA", (SIDE, SIDE), (0, 0, 0, 0))
    canvas.paste(drawn, ((SIDE - drawn.width) // 2, (SIDE - drawn.height) // 2), drawn)
    canvas.save(OUTPUT)
    print(f"{OUTPUT.relative_to(ROOT)}: {SIDE}x{SIDE}, mark {drawn.width}x{drawn.height}")


if __name__ == "__main__":
    main()
