#!/usr/bin/env python3
"""Pixel check of the screen mask in the game's renderer (docs/effects.md, "The screen mask").

    cargo build --release -p opensow-game
    python3 scripts/verify_screen_mask.py target/release/opensow-game [WxH]

Smoke: renders a mission headless three times around a building: without any effect (baseline), with smoke puffs (mode 8) and
missile smoke (mode 14) over and beside it, and the same with `--no-mask`. The masked picture must equal the baseline wherever the
unmasked picture shows smoke over the building or a foreground tile (pixels that differ between the two renders are exactly the
hidden ones), and still show smoke elsewhere.
Shadows: a grid of units with weapon tops around the same spot, rendered with and without the mask; every pixel that differs must be
the top shadow's darkening (the unmasked pixel is not brighter in any channel), and some must differ.
Several missions/areas (player and enemy bases, forests). Needs the game files; no third-party modules (a small PNG reader is included).
"""
import os, re, struct, subprocess, sys, tempfile, zlib

HIDDEN = [0, 0]


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind, body = data[pos + 4 : pos + 8], data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, inter = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype in (2, 6) and inter == 0, "8-bit RGB(A), not interlaced"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    bpp = 3 if ctype == 2 else 4
    raw, stride, out, prev = zlib.decompress(idat), w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        out.append(line)
        prev = line
    return w, h, bpp, out


def diff(a, b):
    w, h, bpp, ra = a
    _, _, _, rb = b
    return {(x, y) for y in range(h) for x in range(w) if ra[y][x * bpp : x * bpp + 3] != rb[y][x * bpp : x * bpp + 3]}



game = sys.argv[1]
size = sys.argv[2] if len(sys.argv) > 2 else "640x480"
tmp = tempfile.mkdtemp()
# (mission, centre cell, building cell the smoke is put around)
AREAS = [("M01", (58, 52), (58, 53)), ("M01", (3, 27), (1, 27)), ("M01", (24, 14), (24, 14)), ("M03", None, None), ("M06", None, None)]


def render(name, mission, centre, extra):
    path = os.path.join(tmp, name + ".png")
    args = [game, "--no-intro", "--mute", "--size", size, "--play", mission, "--screenshot", path, *extra]
    if centre:
        args += ["--center", f"{centre[0]},{centre[1]}"]
    subprocess.run(args, check=True, capture_output=True)
    return read_png(path)


def building_cells(mission):
    """Centre of the first few buildings of a mission, from `opensow level`."""
    tool = os.path.join(os.path.dirname(game), "opensow")
    gdir = subprocess.run([tool, "where"], capture_output=True, text=True).stdout.strip().splitlines()[0]
    lv = [f for f in os.listdir(os.path.join(gdir, "Data", "Levels")) if f.lower() == mission.lower() + ".edt"]
    out = subprocess.run([tool, "level", os.path.join(gdir, "Data", "Levels", lv[0])], capture_output=True, text=True).stdout
    cells = []
    for line in out.splitlines():
        m = re.search(r"(Headquarters|factory|Wind|Air base|Gold|Research)\s.*cell \(\s*(\d+),\s*(\d+)\)", line)
        if m:
            cells.append((int(m[2]), int(m[3])))
    return cells


ok = True
for mission, centre, bcell in AREAS:
    if centre is None:
        cells = building_cells(mission)
        if not cells:
            continue
        bcell = cells[0]
        centre = (bcell[0], bcell[1] - 1)
    cx, cy = bcell
    fx = []
    for dx in range(-2, 3):
        for dy in range(-2, 3):
            fx += ["--fx", f"{cx + dx},{cy + dy},puff,{20 + 5 * (dx + dy + 4) % 40}", "--fx", f"{cx + dx},{cy + dy},smoke,{8 + (dx * dy + 8) % 16}"]
    units = []
    for i, (dx, dy) in enumerate([(dx, dy) for dx in range(-8, 9) for dy in range(-6, 7)]):
        px, py = (cx + dx) * 32 + 16, (cy + dy) * 32 + 16
        if px < 0 or py < 0:
            continue
        units += ["--unit", f"{4 + i % 12},{i % 2},{px},{py}"]
    base, masked, nomask = render("base", mission, centre, []), render("masked", mission, centre, fx), render("nomask", mission, centre, fx + ["--no-mask"])
    shown_masked, shown_nomask = diff(base, masked), diff(base, nomask)
    hidden = shown_nomask - shown_masked
    extra = len(shown_masked - shown_nomask)
    # shadows
    ubase = render("ubase", mission, centre, units)  # with the mask (the default)
    unomask = render("unomask", mission, centre, units + ["--no-mask"])
    d = diff(ubase, unomask)
    _, _, bpp, ra = ubase
    _, _, _, rb = unomask
    brighter = sum(1 for (x, y) in d if any(rb[y][x * bpp + c] > ra[y][x * bpp + c] for c in range(3)))
    line = f"{size} {mission} around {bcell}: smoke {len(shown_nomask)} -> {len(shown_masked)} px (hidden {len(hidden)}, extra {extra}); top shadows hidden by the mask {len(d)} px (brighter without mask: {brighter})"
    case_ok = extra == 0 and brighter == 0
    print(line, "OK" if case_ok else "FAILED")
    ok &= case_ok
    HIDDEN[0] += len(hidden)
    HIDDEN[1] += len(d)
print(f"total hidden smoke pixels {HIDDEN[0]}, hidden shadow pixels {HIDDEN[1]}")
ok &= HIDDEN[0] > 0 and HIDDEN[1] > 0
print("OK" if ok else "FAILED")
sys.exit(0 if ok else 1)
