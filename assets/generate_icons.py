"""
One-shot icon generator.

Run from the repo root:
    python assets/generate_icons.py

Reads assets/rsicon.png (the high-res source) and writes:
    assets/rsicon.ico      Multi-resolution Windows icon (16/32/48/64/128/256)
                           embedded into both .exes via build.rs.
    assets/rsicon-64.png   64x64 PNG used at runtime by the Bevy launcher
                           to set the window icon.

Re-run this whenever the source PNG changes. The generated files are
committed to the repo so contributors don't need Pillow to build.
"""

from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent
SRC = ROOT / "rsicon.png"
ICO_OUT = ROOT / "rsicon.ico"
PNG64_OUT = ROOT / "rsicon-64.png"

ICO_SIZES = [16, 32, 48, 64, 128, 256]


def main() -> None:
    if not SRC.exists():
        raise SystemExit(f"missing source image: {SRC}")

    src = Image.open(SRC).convert("RGBA")

    # Multi-res .ico. Pillow generates each size by downscaling the source.
    src.save(
        ICO_OUT,
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
    )
    print(f"wrote {ICO_OUT} ({', '.join(f'{s}x{s}' for s in ICO_SIZES)})")

    # 64x64 PNG for the runtime window icon. Big enough to look crisp on
    # the taskbar at most DPIs without bloating the binary.
    src.resize((64, 64), Image.LANCZOS).save(PNG64_OUT, format="PNG")
    print(f"wrote {PNG64_OUT} (64x64)")


if __name__ == "__main__":
    main()
