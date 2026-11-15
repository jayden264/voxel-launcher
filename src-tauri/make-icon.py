"""Draws the Voxel app icon as real pixel art (a 32x32 grid) and writes every size the app needs.

Run from src-tauri:  python make-icon.py
The art is drawn by rules (faces, rim light, dithering), so a size change or colour tweak is a one-line edit.
Whole-number scales (x1, x2, x4, x8, x32) keep the pixels razor sharp; odd sizes are filtered down from the 32 grid.
"""

import io
import random
import struct
from pathlib import Path

from PIL import Image

N = 32
ICONS = Path(__file__).parent / "icons"

# Palette: the launcher's green-to-teal accent, on a dark glass tile.
TILE_TOP, TILE_BOTTOM = (28, 42, 56), (10, 16, 23)
TILE_RIM_LIGHT, TILE_RIM_DARK = (58, 82, 104), (6, 10, 14)
OUTLINE = (7, 56, 46)
# Three flat tones per face, light to dark from the top edge down (hard bands: that is what makes it read as pixel art).
FACE = {
    "top": [(176, 250, 204)],
    "left": [(52, 208, 132), (40, 181, 116), (30, 154, 102)],
    "right": [(29, 179, 166), (21, 154, 148), (15, 126, 128)],
}
RIM = {"top": (232, 255, 240), "left": (150, 246, 190), "right": (110, 232, 220)}
TOP_SHADE = (118, 232, 170)  # lower-right edges of the top face
CREASE = (9, 86, 84)
SPARK, SPARK_SOFT = (240, 255, 248), (170, 250, 210)

BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def in_tile(x, y, r=6.5):
    """Rounded square with stepped (pixel) corners."""
    cx = min(max(x + 0.5, r), N - r)
    cy = min(max(y + 0.5, r), N - r)
    return (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2 <= r * r


def face_of(px, py):
    """Which face of the isometric cube covers this pixel centre, if any."""
    # 2:1 isometric: half-width 12, half-height 6, side height 12, cube centred on the tile.
    cx, cy, hw, hh, side = 16.0, 10.0, 12.0, 6.0, 12.0
    if abs(px - cx) / hw + abs(py - cy) / hh <= 1:
        return "top"
    if cx - hw <= px < cx:
        top = cy + (px - (cx - hw)) * hh / hw
        if top <= py <= top + side:
            return "left"
    if cx <= px <= cx + hw:
        top = cy + hh - (px - cx) * hh / hw
        if top <= py <= top + side:
            return "right"
    return None


def draw():
    img = [[(0, 0, 0, 0)] * N for _ in range(N)]
    faces = [[face_of(x + 0.5, y + 0.5) for x in range(N)] for y in range(N)]

    # Tile: vertical gradient in 5 hard bands (pixel look), lit rim top-left, shaded rim bottom-right.
    for y in range(N):
        band = min(4, y * 5 // N) / 4
        for x in range(N):
            if not in_tile(x, y):
                continue
            c = lerp(TILE_TOP, TILE_BOTTOM, band)
            # Soft green glow behind the cube, in two hard steps.
            d = ((x - 16) ** 2 + (y - 17) ** 2) ** 0.5
            if d < 11:
                c = lerp(c, (40, 190, 130), 0.16)
            elif d < 14:
                c = lerp(c, (40, 190, 130), 0.07)
            edge = not all(in_tile(nx, ny) for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)) if 0 <= nx < N and 0 <= ny < N) or x in (0, N - 1) or y in (0, N - 1)
            if edge:
                c = TILE_RIM_LIGHT if (x + y) < N else TILE_RIM_DARK
            img[y][x] = (*c, 255)

    def inside(x, y):
        return 0 <= x < N and 0 <= y < N and faces[y][x] is not None

    # Outline first (so the cube reads against the tile), then the faces over it.
    for y in range(N):
        for x in range(N):
            if faces[y][x] is None and any(inside(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                img[y][x] = (*OUTLINE, 255)

    rng = random.Random(11)

    def top_edge(f, x):
        """y of the face's upper edge at column x (for banding the side faces)."""
        if f == "left":
            return 10.0 + (x + 0.5 - 4.0) * 0.5
        return 16.0 - (x + 0.5 - 16.0) * 0.5

    # Block texture: little 2x2 clusters of darker pixels on the sides, lighter specks on top.
    clusters = {(rng.randrange(5, 27), rng.randrange(14, 27)) for _ in range(9)}
    texture = {(x + dx, y + dy) for x, y in clusters for dx in (0, 1) for dy in (0, 1)}
    specks = {(rng.randrange(9, 24), rng.randrange(5, 16)) for _ in range(7)}

    for y in range(N):
        for x in range(N):
            f = faces[y][x]
            if f is None:
                continue
            near_open = any(not inside(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))
            if f == "top":
                c = FACE["top"][0]
                if (x, y) in specks:
                    c = lerp(c, (255, 255, 255), 0.35)
                if near_open:
                    # Lit edges face the light (upper left); the far edge is a notch darker.
                    c = RIM["top"] if (x + y) < 26 else TOP_SHADE
            else:
                tones = FACE[f]
                dist = y + 0.5 - top_edge(f, x)
                k = min(2, max(0, int(dist * 3 / 12)))
                c = tones[k]
                # A checker row where one band hands over to the next softens the step without muddying it.
                frac = dist * 3 / 12 - k
                if k > 0 and frac < 0.22 and (x + y) % 2 == 0:
                    c = tones[k - 1]
                if (x, y) in texture:
                    c = lerp(c, (0, 52, 52), 0.26)
                if dist < 1.0:
                    c = RIM[f]
                if f == "right" and x == 16:
                    c = CREASE
                if f == "left" and x == 15:
                    c = lerp(c, CREASE, 0.5)
            img[y][x] = (*c, 255)

    # Sparkle (a plus) top-right, and a single lit pixel bottom-left.
    for dx, dy in ((0, 0),):
        img[5][27] = (*SPARK, 255)
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        img[5 + dy][27 + dx] = (*SPARK_SOFT, 255)
    img[26][4] = (*SPARK_SOFT, 255)

    out = Image.new("RGBA", (N, N))
    out.putdata([p for row in img for p in row])
    return out


def scaled(art, size):
    if size % N == 0:
        return art.resize((size, size), Image.NEAREST)  # exact pixel multiples stay razor sharp
    big = art.resize((N * 8, N * 8), Image.NEAREST)  # odd sizes: filter down from a clean 8x version
    return big.resize((size, size), Image.LANCZOS)


def ico(art, sizes):
    """An .ico with PNG-encoded images at the exact sizes (Pillow's own writer would resample them)."""
    blobs = []
    for s in sizes:
        buf = io.BytesIO()
        scaled(art, s).save(buf, "PNG")
        blobs.append(buf.getvalue())
    head = struct.pack("<HHH", 0, 1, len(sizes))
    entries, offset = b"", 6 + 16 * len(sizes)
    for s, b in zip(sizes, blobs):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(b), offset)
        offset += len(b)
    return head + entries + b"".join(blobs)


def main():
    art = draw()
    ICONS.mkdir(exist_ok=True)
    scaled(art, 1024).save(ICONS / "icon.png")
    for name, size in {
        "32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256,
        "Square30x30Logo.png": 30, "Square44x44Logo.png": 44, "Square71x71Logo.png": 71, "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107, "Square142x142Logo.png": 142, "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284, "Square310x310Logo.png": 310, "StoreLogo.png": 50,
    }.items():
        scaled(art, size).save(ICONS / name)
    (ICONS / "icon.ico").write_bytes(ico(art, [16, 24, 32, 48, 64, 128, 256]))
    scaled(art, 512).save(Path(__file__).parent / "icon-preview-512.png")
    print("icons written to", ICONS)


if __name__ == "__main__":
    main()
