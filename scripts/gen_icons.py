#!/usr/bin/env -S uv run --quiet
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "resvg-py>=0.1",
#   "pillow>=10.2",
# ]
# ///
"""
Generate the full SpeakIn icon set from a single SVG source.

Each output size is rendered DIRECTLY from vector at the target pixel
dimensions — no master-bitmap downsampling. This is what makes the
taskbar / tray icons crisp instead of fuzzy.

Two-tier icon strategy:
  • Main SVG     — detailed/airy version, used for large displays
                   (Store logos, ICNS, About dialog, large file icons)
  • Tray SVG     — simplified high-contrast version, used for tiny sizes
                   (system tray, taskbar, ICO layers ≤ 48px)

The tray SVG is auto-detected at:
    1. <out_dir>/tray.svg          (project-local, takes precedence)
    2. <main_svg_dir>/tray.svg     (alongside the design source)
…or pass `--tray-svg PATH` explicitly. If no tray SVG is found, the main
SVG is used for everything.

Usage:
    # Default — auto-detect tray.svg in src-tauri/icons/
    uv run scripts/gen_icons.py path/to/icon.svg

    # Custom output directory
    uv run scripts/gen_icons.py path/to/icon.svg --out src-tauri/icons

    # Explicit tray SVG override
    uv run scripts/gen_icons.py path/to/icon.svg --tray-svg path/to/tray.svg

    # Skip tray entirely (no tray PNGs, ICO uses main SVG only)
    uv run scripts/gen_icons.py path/to/icon.svg --skip-tray
"""

from __future__ import annotations

import argparse
import sys
from io import BytesIO
from pathlib import Path

import resvg_py
from PIL import Image

# ── Tauri standard PNG set + Windows Store square logos ─────────────
PNG_OUTPUTS: dict[str, int] = {
    # Tauri runtime / packaging set
    "32x32.png": 32,
    "64x64.png": 64,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    # Windows Store square logos (used by MSIX bundle)
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}

# Tray icons (separate so you can later swap to a tray-specific SVG)
TRAY_OUTPUTS: dict[str, int] = {
    "tray.png": 32,
    "tray@2x.png": 64,
}

# ICO embeds multiple resolutions; Windows picks per-DPI at runtime.
# Sizes <= ICO_SMALL_THRESHOLD use the tray (simplified) SVG when available;
# larger sizes use the main detailed SVG. This mirrors how Office/system
# icons are designed — small variants strip details that turn to mush.
ICO_SIZES: list[int] = [16, 20, 24, 32, 40, 48, 64, 128, 256]
ICO_SMALL_THRESHOLD: int = 48

# ICNS supported types (Pillow maps these to ic07/ic08/.../ic14)
ICNS_SIZES: list[int] = [16, 32, 64, 128, 256, 512, 1024]


def render_png(svg_text: str, size: int) -> Image.Image:
    """Vector-rasterize the SVG at exact pixel dimensions via resvg."""
    png_bytes = resvg_py.svg_to_bytes(
        svg_string=svg_text,
        width=size,
        height=size,
    )
    return Image.open(BytesIO(bytes(png_bytes))).convert("RGBA")


def write_pngs(svg_text: str, out: Path, table: dict[str, int], label: str) -> None:
    print(f"\n[{label}]")
    for filename, size in table.items():
        img = render_png(svg_text, size)
        path = out / filename
        img.save(path, "PNG", optimize=True)
        print(f"  {filename:30}  {size:>4}x{size:<4}")


def write_ico(main_svg_text: str, tray_svg_text: str | None, out: Path) -> None:
    """Save multi-size ICO with each layer pre-rendered from vector.

    Mixed-source layers: small sizes (<= ICO_SMALL_THRESHOLD) come from the
    tray/simplified SVG when one is provided, large sizes from the main SVG.

    Pillow's ICO encoder uses the base image's dimensions as a ceiling and
    skips any requested sizes larger than the base, so the base must be the
    LARGEST layer; smaller layers go in append_images. Pillow then matches
    each entry in `sizes` against the provided images by exact dimensions.
    """
    print("\n[ICO]")
    sizes_desc = sorted(ICO_SIZES, reverse=True)  # largest first
    layers: list[Image.Image] = []
    for size in sizes_desc:
        if tray_svg_text is not None and size <= ICO_SMALL_THRESHOLD:
            layers.append(render_png(tray_svg_text, size))
            tag = "tray"
        else:
            layers.append(render_png(main_svg_text, size))
            tag = "main"
        print(f"  layer {size:>3}x{size:<3}  ← {tag}")
    base, *rest = layers
    base.save(
        out / "icon.ico",
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=rest,
    )
    print(f"  icon.ico                       sizes: {ICO_SIZES}")


def write_icns(svg_text: str, out: Path) -> None:
    """Save multi-size ICNS. Same constraint as ICO — base must be largest."""
    print("\n[ICNS]")
    sizes_desc = sorted(ICNS_SIZES, reverse=True)
    layers = [render_png(svg_text, s) for s in sizes_desc]
    base, *rest = layers
    try:
        base.save(
            out / "icon.icns",
            format="ICNS",
            append_images=rest,
        )
        print(f"  icon.icns                      sizes: {ICNS_SIZES}")
    except Exception as e:
        # Pillow's ICNS writer can be picky on some platforms.
        # Fall back to a single high-res entry — macOS will scale it.
        print(f"  multi-size ICNS failed ({e}); writing 1024-only fallback")
        base.save(out / "icon.icns", format="ICNS")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("svg", type=Path, help="Source SVG path")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("src-tauri/icons"),
        help="Output directory (default: src-tauri/icons, relative to repo root)",
    )
    parser.add_argument(
        "--skip-tray",
        action="store_true",
        help="Skip tray.png / tray@2x.png generation",
    )
    parser.add_argument(
        "--tray-svg",
        type=Path,
        default=None,
        help=(
            "Optional separate SVG used only for tray icons. "
            "Designed for tiny sizes (16-32px) — typically no filters, "
            "no thin strokes, high-contrast solid fills."
        ),
    )
    args = parser.parse_args()

    if not args.svg.exists():
        print(f"error: SVG not found: {args.svg}", file=sys.stderr)
        return 1

    out = args.out
    out.mkdir(parents=True, exist_ok=True)

    svg_text = args.svg.read_text(encoding="utf-8")

    # Resolve tray SVG: explicit --tray-svg wins; otherwise auto-detect
    # `tray.svg` next to the output dir, or next to the main SVG.
    tray_svg_path: Path | None = None
    if args.tray_svg is not None:
        if not args.tray_svg.exists():
            print(f"error: tray SVG not found: {args.tray_svg}", file=sys.stderr)
            return 1
        tray_svg_path = args.tray_svg
    else:
        for candidate in (out / "tray.svg", args.svg.parent / "tray.svg"):
            if candidate.exists():
                tray_svg_path = candidate
                break

    tray_svg_text: str | None = None
    if tray_svg_path is not None:
        tray_svg_text = tray_svg_path.read_text(encoding="utf-8")

    # --skip-tray means "no tray styling at all" — also disable the simplified
    # source for ICO small layers, otherwise the flag's behavior is surprising.
    if args.skip_tray:
        tray_svg_text = None
        tray_svg_path = None

    print(f"source     : {args.svg}")
    if tray_svg_path is not None:
        print(f"tray source: {tray_svg_path}")
    else:
        print("tray source: (none — main SVG will be used for tray + small ICO layers)")
    print(f"target     : {out.resolve()}")

    write_pngs(svg_text, out, PNG_OUTPUTS, "PNG / Windows Store")
    if not args.skip_tray:
        # Tray PNGs always render from the simplified SVG when available.
        write_pngs(tray_svg_text or svg_text, out, TRAY_OUTPUTS, "Tray")
    write_ico(svg_text, tray_svg_text, out)
    write_icns(svg_text, out)

    print("\n✓ done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
