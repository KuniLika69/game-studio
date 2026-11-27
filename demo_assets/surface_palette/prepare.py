"""Prepare the locally vendored Poly Haven CC0 scan maps for Robot Studio."""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent
SCANS = ROOT.parent / "rain" / "scans"
SIZE = (1024, 1024)
SETS = {
    "forest": "forest_ground_06",
    "pebbles": "dry_river_pebbles",
    "paving": "cobblestone_pavement",
    "brick": "brick_wall_09",
}

for name, source in SETS.items():
    for kind, original in (("color", "diff"), ("normal", "nor_gl")):
        with Image.open(SCANS / f"{source}-{original}.jpg") as source_map:
            prepared = source_map.convert("RGB").resize(SIZE, Image.Resampling.LANCZOS)
            prepared.save(ROOT / f"{name}-{kind}.jpg", quality=86, subsampling=0, optimize=True)
    with Image.open(SCANS / f"{source}-rough.jpg") as source_map:
        roughness = source_map.convert("L").resize(SIZE, Image.Resampling.LANCZOS)
        red = Image.new("L", SIZE, 255)
        blue = Image.new("L", SIZE, 0)
        Image.merge("RGB", (red, roughness, blue)).save(ROOT / f"{name}-rough.png", optimize=True)
