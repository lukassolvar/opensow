#!/usr/bin/env python3
"""Run functions of the original State of War.exe in an x86 emulator and compare them with OpenSOW's Rust ports.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_against_exe.py "<install>/State of War.exe" target/release/opensow

The exe is only read, never modified or copied. Checks (random inputs, exact comparison):
  * 0x40ef60  bullet path builder          vs `opensow path`      (bullet types 1..35 except 13/14; type 1 also compares the missile facing [+0x2654])
  * 0x411dc0  beam-bullet spark loops (types 19-21) + dim end frame (23), visual RNG hooked   vs `opensow sparks`
  * 0x436df0  speed table builder          vs `opensow speedrow`
  * 0x46b180  heading -> facing            vs `opensow facing`    (32, 16, 8 and unsupported step counts)
  * 0x46a190  one turn step                vs `opensow turn`
  * 0x467ee0  aircraft launch (fields)     vs `opensow launch`    (all five air forces, random targets)
  * 0x468960  carryall squad unload        vs `opensow squad`     (indices 0..17; unit-creation call recorded)
  * 0x46bc70  Advancer hover table       vs `opensow hover`     (42 phases)
  * 0x4753a0  wave pathfinder              vs `opensow wave`      (random grids, 300 searches incl. failures; every second
                                           search runs the air layer 0x474b30)
  * 0x46ee80  walking pixel line           vs `opensow line`      (300 random segments)
  * 0x476870  local avoidance search       vs `opensow local`     (300 random grids with units and buildings; every second
                                           case is the air layer 0x473f10)
  * 0x475c30  second-step local search    vs `opensow local2`  (300 random grids; every second case is the air layer 0x4732f0)
  * 0x4774c0  free cell near the destination (+ flood 0x4726f0) vs `opensow freecell` (300 random grids; every second case is air)
  * 0x46fa40  networked chassis step (position from the game clock) vs `opensow netstep` (300 cases, callees stubbed)
  * 0x477630  chase cell search            vs `opensow approach` (300 random grids, unit targets; every second case is
                                           the air chase 0x478200 via handle type 4, `approach ... air`)
  * 0x472a90  target reachability scan    vs `opensow targets` (300 random grids with unit/turret/building handles)
  * 0x47c6b0  Advancer beam cooldown tick vs `opensow beamtick` (states 0..7, random counters/reloads)
  * 0x45d3a0 / 0x45d400  pause and mission-end frame rows vs `opensow scanlines` (565 and 555 branches, random sizes and pixels)
  * 0x436230  range ring list builder      vs `opensow rings`     (64-entry index table + 14017 offsets)
  * 0x40f2c0  random curve table (bullet types 13/14) + their constructor/path draws   vs `opensow curves|curvebullet`
  * 0x40aa04..0x40ace6 constructor turret spots vs `opensow turretspots`; 0x431120 the computer's turret routine vs `opensow turretai`
                                           (1500 random worlds; rand, placement test, creator stubbed)
`atan` (an import) is emulated by a hook; nothing else outside the image is needed.
"""
import math, random, struct, subprocess, sys

import pefile
from unicorn import UC_ARCH_X86, UC_HOOK_CODE, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_XMM0

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path)
base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, BULLET, RET, ATAN = 0x70000000, 0x60000000, 0x50000000, 0x47EDEA
mu.mem_map(STACK, 0x100000)
CALLSTACK = 0x72000000
mu.mem_map(CALLSTACK, 0x200000)
mu.mem_map(BULLET, 0x10000)
mu.mem_map(RET, 0x1000)
mu.mem_write(RET, b"\xf4")


def atan_hook(uc, addr, size, ud):
    lo = uc.reg_read(UC_X86_REG_XMM0) & ((1 << 64) - 1)
    x = struct.unpack("<d", struct.pack("<Q", lo))[0]
    uc.reg_write(UC_X86_REG_XMM0, struct.unpack("<Q", struct.pack("<d", math.atan(x)))[0])
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, atan_hook, begin=ATAN, end=ATAN)


def call(addr, args, ecx=None):
    sp = CALLSTACK + 0x200000 - 0x1000  # the walkers' routines nest in their dispatchers: frames of 2 x 0x60124 bytes
    for a in reversed(args):
        sp -= 4
        mu.mem_write(sp, struct.pack("<i", a))
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    if ecx is not None:
        mu.reg_write(UC_X86_REG_ECX, ecx)
    mu.emu_start(addr, RET, count=2_000_000)
    return mu.reg_read(UC_X86_REG_EAX)


def sh(*cmd):
    return subprocess.run([opensow, *map(str, cmd)], capture_output=True, text=True).stdout.strip()


failures = 0


def report(name, total, bad):
    global failures
    failures += bad
    print(f"{name}: {total} compared, {bad} mismatches")


# 0c. The computer's turret routine (runs first, on clean memory: later sections hook the handle functions and the generator): the constructor's eight turret spots (0x40aa04..0x40ace6) vs `opensow turretspots`, and the whole
#     routine 0x431120 (rand 0x437b10, the placement test 0x408be0, the building creator 0x439e30 and the checksum 0x4062a0 stubbed;
#     everything else real) vs `opensow turretai`.
import os, tempfile
OBJ = 0x64000000
mu.mem_map(OBJ, 0x200000)
CTOR_FRAME = STACK + 0x80000


def ctor_spots(kind, x, y, obj):
    """Runs the constructor part that computes the spots on a fresh building object; returns the 16 ints at +0x224."""
    mu.mem_write(obj, b"\0" * 0x400)
    mu.mem_write(obj + 4, struct.pack("<ii", x, y))
    mu.mem_write(obj + 0x94, struct.pack("<i", kind))
    from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP
    mu.reg_write(UC_X86_REG_EBP, CTOR_FRAME)
    mu.reg_write(UC_X86_REG_ESP, CTOR_FRAME - 0x400)
    mu.reg_write(UC_X86_REG_EBX, obj)
    mu.reg_write(UC_X86_REG_ESI, kind)
    mu.reg_write(UC_X86_REG_EDI, 0)
    mu.emu_start(0x40AA04, 0x40ACE6, count=100000)
    return list(struct.unpack("<16i", mu.mem_read(obj + 0x224, 64)))


bad = 0
cases = 0
for kind in (100, 101, 102, 103, 104, 105, 106, 107):
    for _ in range(25):
        x, y = random.randint(2, 100), random.randint(2, 100)
        got = ctor_spots(kind, x, y, OBJ)
        want = [int(v) for v in sh("turretspots", kind, x, y).split()]
        # the exe stores x_i at +0x224+4i and y_i at +0x244+4i
        xs = struct.unpack("<8i", mu.mem_read(OBJ + 0x224, 32))
        ys = struct.unpack("<8i", mu.mem_read(OBJ + 0x244, 32))
        cases += 1
        bad += list(xs) + list(ys) != want
report("turret spots written by the building constructor (0x40aa04..0x40ace6)", cases, bad)

from unicorn.x86_const import UC_X86_REG_EIP as _EIP, UC_X86_REG_ESP as _ESP, UC_X86_REG_EAX as _EAX

HOOK = {"rand": [], "log": [], "place": []}


def _ret(uc):
    sp = uc.reg_read(_ESP)
    (r,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(_ESP, sp + 4)
    uc.reg_write(_EIP, r)


def _arg(uc, i):
    sp = uc.reg_read(_ESP)
    return struct.unpack("<i", uc.mem_read(sp + 4 + 4 * i, 4))[0]


def h_rand(uc, a, s, ud):
    n = _arg(uc, 0)
    seq = HOOK["seq"]
    v = seq[HOOK["n"] % len(seq)]
    HOOK["n"] += 1
    uc.reg_write(_EAX, v % n if n else 0)
    _ret(uc)


def h_test(uc, a, s, ud):
    x, y, w, h = (_arg(uc, i) for i in range(4))
    okp = _arg(uc, 6) & 0xFFFFFFFF
    ok = ((x * 31 + y * 17) & 7) < 3
    HOOK["log"].append(f"try {x} {y} {w} {h}")
    uc.mem_write(okp, b"\x01" if ok else b"\x00")
    _ret(uc)


def h_place(uc, a, s, ud):
    x, y, kind = _arg(uc, 0), _arg(uc, 1), _arg(uc, 3)  # (x, y, player, kind, 0, 0)
    HOOK["place"].append((x, y, kind))
    _ret(uc)


def h_nop(uc, a, s, ud):
    _ret(uc)


TURRET_HOOKS = [mu.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr) for addr, fn in ((0x437B10, h_rand), (0x408BE0, h_test), (0x439E30, h_place), (0x4062A0, h_nop))]

GRID = 0x5FAC38
BLD_TABLE, UNIT_TABLE, BLD_COUNT, UNIT_COUNT, CREDITS, FLAGS = 0x5BAB00, 0x5B62C0, 0x575740, 0x575734, 0x517AF8, 0x70BAF5
UNIT_OBJ = OBJ + 0x100000

bad = 0
cases = 0
placed = 0
TIERS = {}
for case in range(1500):
    me = random.randint(0, 1)
    nb = random.randint(1, 4)
    blds = []
    for i in range(nb):
        kind = random.choice([100, 101, 102, 103, 104, 105, 106, 107, 107, 25, 26, 28])
        x, y = random.randint(6, 90), random.randint(6, 90)
        obj = OBJ + 0x1000 * i
        if kind >= 100:
            ctor_spots(kind, x, y, obj)
            w, h, _ = struct.unpack("<iii", mu.mem_read(obj + 0x14, 4) + mu.mem_read(obj + 0x18, 4) + b"\0" * 4)
        else:  # a turret: skipped by the routine, only its kind matters
            mu.mem_write(obj, b"\0" * 0x400)
            mu.mem_write(obj + 4, struct.pack("<ii", x, y))
            mu.mem_write(obj + 0x94, struct.pack("<i", kind))
            mu.mem_write(obj + 0x14, struct.pack("<ii", 1, 1))
        blds.append((kind, x, y, obj))
        mu.mem_write(BLD_TABLE + 4 * (me * 0x12C + i), struct.pack("<I", obj))
    mu.mem_write(BLD_COUNT + 4 * me, struct.pack("<i", nb))
    # units: three players, owner == table
    lines = [f"me {me}"]
    uidx = 0
    for q in range(3):
        cnt = random.randint(0, 6)
        for j in range(cnt):
            kind_, bx, by, _ = random.choice(blds)
            cx, cy = bx + random.randint(-7, 8), by + random.randint(-7, 8)
            ukind = random.choice([1, 2, 7, 11, 40, 41, 43, 45, 46, 109, 110, 111, 112, 113])
            if ukind == 40 and random.random() < 0.7 and kind_ >= 100:
                o = random.choice([b for b in blds if b[0] >= 100])
                ax = call(0x409AB0, [], ecx=o[3]) & 0xFFFFFFFF
                ay = call(0x409AC0, [], ecx=o[3]) & 0xFFFFFFFF
                cx, cy = struct.unpack("<i", struct.pack("<I", ax))[0], struct.unpack("<i", struct.pack("<I", ay))[0]
            u = UNIT_OBJ + 0x100 * uidx
            uidx += 1
            px, py = cx * 32 + random.randint(0, 31), cy * 32 + random.randint(0, 31)
            mu.mem_write(u, b"\0" * 0x100)
            mu.mem_write(u + 0x14, struct.pack("<ii", px, py))
            mu.mem_write(u + 0x40, struct.pack("<i", ukind))
            mu.mem_write(u + 0x44, struct.pack("<i", q))
            mu.mem_write(UNIT_TABLE + 4 * (q * 1000 + j), struct.pack("<I", u))
            lines.append(f"u {cx} {cy} {q} {ukind}")
        mu.mem_write(UNIT_COUNT + 4 * q, struct.pack("<i", cnt))
    credits = random.choice([150, 250, 450, 700, 900, 1000, 1200, 1500, 1600, 2500, 6000])
    flags = [random.random() < 0.6 for _ in range(5)]
    mu.mem_write(CREDITS + 4 * me, struct.pack("<i", credits))
    mu.mem_write(FLAGS + me * 0xC68, bytes(int(f) for f in flags))
    # turret handles on the grid around the spots: class 2 (bits 22-24), type 6 = anti-air (bits 19-21), type 3 = other turrets
    touched = []
    for kind, x, y, obj in blds:
        if kind < 100:
            continue
        xs = struct.unpack("<8i", mu.mem_read(obj + 0x224, 32))
        ys = struct.unpack("<8i", mu.mem_read(obj + 0x244, 32))
        for sx, sy in zip(xs, ys):
            r = random.random()
            if r < 0.3 and 0 <= sx < 128 and 0 <= sy < 128:
                t = 6 if r < 0.2 else 3
                mu.mem_write(GRID + 4 * (sx * 128 + sy), struct.pack("<I", (2 << 22) | (t << 19) | 0x123))
                touched.append((sx, sy))
                if t == 6:
                    lines.append(f"aa {sx} {sy}")
    for kind, x, y, obj in blds:
        if kind >= 100:
            lines.append(f"b {kind} {x} {y}")
    lines += [f"credits {credits}", "flags " + " ".join(str(int(f)) for f in flags)]
    seq = [random.randrange(65536) for _ in range(12)]
    lines.append("rng " + " ".join(map(str, seq)))
    HOOK.update(seq=seq, n=0, log=[], place=[])
    call(0x431120, [me])
    got = HOOK["log"] + ([f"place {x} {y} {k}" for x, y, k in HOOK["place"]] or ["none"])
    (after,) = struct.unpack("<i", mu.mem_read(CREDITS + 4 * me, 4))
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
        f.write("\n".join(lines))
    want = sh("turretai", f.name).split("\n")
    os.unlink(f.name)
    cases += 1
    placed += got[-1] != "none"
    if got[-1] != "none":
        TIERS[int(got[-1].split()[3]) - 24] = TIERS.get(int(got[-1].split()[3]) - 24, 0) + 1
    if got != want or (got[-1] != "none" and after != credits - 200 * (int(got[-1].split()[3]) - 24)):
        bad += 1
        if bad <= 3:
            print("MISMATCH", lines, "\n exe ", got, after, "\n rust", want)
    for sx, sy in touched:
        mu.mem_write(GRID + 4 * (sx * 128 + sy), b"\0\0\0\0")
report(f"computer turret routine (0x431120, {placed} placements, tiers {sorted(TIERS.items())}; rand/placement test/creator stubbed)", cases, bad)
for _h in TURRET_HOOKS:
    mu.hook_del(_h)


# 0. Advancer beam cooldown member (0x47c6b0 tick; object fields: +4 enabled, +0x1c counter, +0x20 reload, +0x40 state)
bad = 0
for _ in range(1500):
    st, cnt, rl = random.randint(0, 7), random.randint(-3, 60), random.choice([1, 4, 10, 10, 10, 25])
    mu.mem_write(BULLET, b"\0" * 0x100)
    mu.mem_write(BULLET + 4, b"\x01")
    mu.mem_write(BULLET + 0x1C, struct.pack("<iii", cnt, rl, 0))
    mu.mem_write(BULLET + 0x40, struct.pack("<i", st))
    call(0x47C6B0, [], ecx=BULLET)
    c, s_ = struct.unpack("<i", mu.mem_read(BULLET + 0x1C, 4))[0], struct.unpack("<i", mu.mem_read(BULLET + 0x40, 4))[0]
    bad += f"{s_} {c}" != sh("beamtick", st, cnt, rl)
report("beam cooldown tick (0x47c6b0)", 1500, bad)

# 0b. frame effects of the pause (0x45d3a0) and of the mission-end banner (0x45d400; globals: width [0x49a340], height [0x49a344],
#     pixel format [0x517aec] == 2 is RGB565, anything else the 555 branch)
bad = 0
for case in range(300):
    w, h = random.choice([2, 5, 16, 33]), random.choice([2, 4, 7, 10])
    px = [random.randrange(65536) for _ in range(w * h)]
    for mode, fn, fmt in (("pause", 0x45D3A0, 2), ("banner", 0x45D400, 2), ("banner555", 0x45D400, 1)):
        mu.mem_write(base_ := 0x49A340, struct.pack("<ii", w, h))
        mu.mem_write(0x517AEC, struct.pack("<i", fmt))
        mu.mem_write(BULLET, struct.pack(f"<{w * h}H", *px))
        call(fn, [w, BULLET])
        got = struct.unpack(f"<{w * h}H", mu.mem_read(BULLET, 2 * w * h))
        want = [int(v, 16) for v in sh("scanlines", mode, w, h, *[f"{v:04x}" for v in px]).split()]
        bad += list(got) != want
report("pause / banner frame rows (0x45d3a0, 0x45d400)", 900, bad)

# 1. bullet paths
def exe_path_for(t, x1, y1, x2, y2, delay):
    mu.mem_write(BULLET, b"\0" * 0x2700)
    mu.mem_write(BULLET + 0x2658, struct.pack("<I", t))
    call(0x40EF60, [x1, y1, x2, y2, delay], ecx=BULLET)
    (cnt,) = struct.unpack("<I", mu.mem_read(BULLET + 0x2648, 4))
    (first,) = struct.unpack("<I", mu.mem_read(BULLET + 0x2644, 4))
    pts, lifts = [], []
    for i in range(cnt // 2):
        pts.append(struct.unpack("<ii", mu.mem_read(BULLET + 0xD44 + 8 * i, 8)))
        lifts.append(struct.unpack("<i", mu.mem_read(BULLET + 0xC4 + 4 * i, 4))[0])
    (face,) = struct.unpack("<I", mu.mem_read(BULLET + 0x2654, 4))
    return cnt // 2, first // 2, pts, lifts, face if t == 1 else None


def rust_path(t, *a):
    out = subprocess.run([opensow, "path", str(t), *map(str, a)], capture_output=True, text=True)
    if out.returncode:
        return None
    lines = out.stdout.split("\n")
    head = list(map(int, lines[0].split()))
    n, f = head[:2]
    rows = [tuple(map(int, l.split())) for l in lines[1 : 1 + n]]
    return n, f, [(r[0], r[1]) for r in rows], [r[2] for r in rows], (head[2] if len(head) > 2 else None)


random.seed(7)
bad = 0
types = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 15, 16, 17, 18, 24, 26, 27, 31, 33, 34, 35]
for _ in range(1500):
    t = random.choice(types)
    x1, y1 = random.randint(0, 3000), random.randint(0, 3000)
    r = random.choice([5, 40, 150, 400, 900, 1400])
    x2, y2 = x1 + random.randint(-r, r), y1 + random.randint(-r, r)
    x2 = x1 if random.random() < 0.1 else x2
    y2 = y1 if random.random() < 0.1 else y2
    delay = random.choice([0, 0, 0, 1, 3, 40, 2000])
    bad += exe_path_for(t, x1, y1, x2, y2, delay) != rust_path(t, x1, y1, x2, y2, delay)
report("bullet paths (0x40ef60)", 1500, bad)

# 1b. random-curve bullets (types 13/14): curve table builder 0x40f2c0, constructor draws 0x411dc0, path copy 0x40ef60.
# (runs before the other sections, which install hooks and change memory.) The lockstep RNG (0x437b10) runs for real on generic.file, mapped at RNGTAB; the index starts at 0 as in the game's init.
import os

gen = os.path.join(os.path.dirname(exe_path), "Data", "generic.file")
if os.path.exists(gen):
    RNGTAB = 0x30000000
    mu.mem_map(RNGTAB, 0x20000)
    mu.mem_write(RNGTAB, open(gen, "rb").read()[: 0xC347 * 2])
    mu.mem_write(0x71C0F0, struct.pack("<I", RNGTAB))
    mu.mem_write(0x71C0F4, struct.pack("<I", 0))
    call(0x40F2C0, [])
    (idx,) = struct.unpack("<I", mu.mem_read(0x71C0F4, 4))
    lens = struct.unpack("<200h", mu.mem_read(0x4EC098, 400))
    exe_lines = []
    for k in range(200):
        n = lens[k] // 2
        exe_lines.append(f"{k} {n}")
        for i in range(n):
            x, y = struct.unpack("<hh", mu.mem_read(0x49DE98 + k * 0x640 + 4 * i, 4))
            (l,) = struct.unpack("<h", mu.mem_read(0x4EC228 + k * 0x320 + 2 * i, 2))
            exe_lines.append(f"{x} {y} {l}")
    rust_lines = sh("curves", os.path.dirname(gen)).split("\n")
    bad = sum(a != b for a, b in zip(exe_lines, rust_lines)) + abs(len(exe_lines) - len(rust_lines))
    report(f"random curve table (0x40f2c0, {len(exe_lines)} lines, rng index after = {idx})", len(exe_lines), bad)
    failures += idx != 600
    # constructor draws + path copy, 300 bullets, continuing the same RNG stream
    bad = 0
    for _ in range(300):
        t = random.choice([13, 14])
        (before,) = struct.unpack("<I", mu.mem_read(0x71C0F4, 4))
        mu.mem_write(BULLET, b"\0" * 0x2700)
        call(0x411DC0, [t, 80, 0, 0, 0, 0, 1], ecx=BULLET)
        speed, flag = struct.unpack("<I", mu.mem_read(BULLET + 0x2664, 4))[0], struct.unpack("<I", mu.mem_read(BULLET + 0x268C, 4))[0]
        x1, y1 = random.randint(0, 3000), random.randint(0, 3000)
        mu.mem_write(BULLET + 0x269C, struct.pack("<iiiI", x1, y1, 0, 0))
        call(0x40EF10, [], ecx=BULLET)
        cnt, first = struct.unpack("<II", mu.mem_read(BULLET + 0x2648, 4)[:4] + mu.mem_read(BULLET + 0x2644, 4))
        pts = [struct.unpack("<ii", mu.mem_read(BULLET + 0xD44 + 8 * i, 8)) for i in range(cnt // 2)]
        lifts = [struct.unpack("<i", mu.mem_read(BULLET + 0xC4 + 4 * i, 4))[0] for i in range(cnt // 2)]
        rust = sh("curvebullet", gen, t, before, x1, y1).split("\n")
        head = rust[0].split()
        ok = head == [str(speed), str(flag if t == 13 else -1), str(cnt // 2)] and rust[1:] == [f"{p[0]} {p[1]} {l}" for p, l in zip(pts, lifts)]
        bad += not ok
        if not ok and bad <= 2:
            print("  curve bullet mismatch", t, before, head, speed, flag, cnt // 2)
    report("random-curve bullets (0x411dc0 draws + 0x40ef10 path, 300)", 300, bad)
else:
    print("random curve checks skipped (no Data/generic.file next to the exe)")


# 1c. beam-bullet sparks (types 19/20/21) and the dim end frame (type 23): constructor 0x411dc0 with the visual generator 0x437cd0
#     hooked to return a scripted value % n (the real one draws from a C rand() table). Arrays: A [+0x0c], B [+0x48], C [+0x84].
RANDFN = 0x437CD0
rand_vals, rand_pos = [], [0]


def rand_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret, n) = struct.unpack("<Ii", uc.mem_read(sp, 8))
    v = rand_vals[rand_pos[0] % len(rand_vals)]
    rand_pos[0] += 1
    uc.reg_write(UC_X86_REG_EAX, v % (n & 0xFFFF) if n & 0xFFFF else 0)
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, rand_hook, begin=RANDFN, end=RANDFN)
bad = 0
for _ in range(400):
    t = random.choice([19, 20, 21, 23])
    variant = random.randint(0, 3)
    rand_vals[:] = [random.randint(0, 65535) for _ in range(40)]
    rand_pos[0] = 0
    mu.mem_write(BULLET, b"\0" * 0x2700)
    mu.mem_write(BULLET + 0x2680, struct.pack("<i", variant))
    call(0x411DC0, [t, 80, 0, 0, 0, 0, 1], ecx=BULLET)
    rust = sh("sparks", t, variant, *rand_vals).split("\n")
    if t == 23:
        (end,) = struct.unpack("<i", mu.mem_read(BULLET + 0x2674, 4))
        ok = rust == [str(end)]
    else:
        (cnt,) = struct.unpack("<i", mu.mem_read(BULLET + 0x268C, 4))
        (delay,) = struct.unpack("<I", mu.mem_read(BULLET + 0x26AC, 4))
        frags = []
        for i in range(cnt):
            a, b, c = (struct.unpack("<i", mu.mem_read(BULLET + off + 4 * i, 4))[0] for off in (0x0C, 0x48, 0x84))
            frags.append(f"{a} {b} {c}")
        ok = rust == [str(delay)] + frags
    bad += not ok
    if not ok and bad <= 2:
        print("  sparks mismatch", t, variant, rust[:3])
report("beam bullet sparks + dim end frame (0x411dc0 cases 19-21, 23; 400)", 400, bad)

# 2. speed table
call(0x436DF0, [])
table = bytes(mu.mem_read(0x5524A0, 2399 * 60))
speeds = list(range(0, 121)) + [150, 179, 180, 181, 210, 300, 359, 360, 599, 600, 1799, 1800, 2000, 2398]
bad = sum(" ".join(str(b) for b in table[v * 60 : v * 60 + 60]) != sh("speedrow", v) for v in speeds)
report("speed table rows (0x436df0)", len(speeds), bad)

# 3. facing
bad = 0
for _ in range(2500):
    steps = random.choice([32, 32, 32, 16, 16, 8, 7])
    ax, ay = random.randint(-300, 3000), random.randint(-300, 3000)
    r = random.choice([1, 5, 40, 300, 2000])
    bx, by = ax + random.randint(-r, r), ay + random.randint(-r, r)
    bx = ax if random.random() < 0.08 else bx
    by = ay if random.random() < 0.08 else by
    e = call(0x46B180, [ax, ay, bx, by, steps]) & 0xFFFFFFFF
    e -= 1 << 32 if e >= 1 << 31 else 0
    bad += str(e) != sh("facing", ax, ay, bx, by, steps)
report("facing (0x46b180)", 2500, bad)

# 4. turn step
bad = 0
CUR = STACK + 0x90000
for _ in range(1500):
    steps = random.choice([32, 16])
    c, t = random.randrange(steps), random.randrange(steps)
    mu.mem_write(CUR, struct.pack("<i", c))
    call(0x46A190, [CUR, t, steps])
    bad += str(struct.unpack("<i", mu.mem_read(CUR, 4))[0]) != sh("turn", c, t, steps)
report("turn step (0x46a190)", 1500, bad)

# 4b. Advancer hover bob table
call(0x46BC70, [])
hover = struct.unpack("<42i", mu.mem_read(0x89CF58, 42 * 4))
report("hover table (0x46bc70)", 1, int(" ".join(map(str, hover)) != sh("hover")))

# 5. range rings: three UI-object helpers are stubbed (they only do bookkeeping for the first rings)
STUBS = {0x47D7FB, 0x47D801, 0x47D807}


def ret_imm(addr):
    code = bytes(mu.mem_read(addr, 200))
    for i in range(len(code) - 2):
        if code[i] == 0xC2:
            return struct.unpack("<H", code[i + 1 : i + 3])[0]
        if code[i] == 0xC3:
            return 0
    return 0


STUB_POP = {a: ret_imm(a) for a in STUBS}


def stub_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + STUB_POP[addr])
    uc.reg_write(UC_X86_REG_EIP, ret)
    uc.reg_write(UC_X86_REG_EAX, 0)


mu.hook_add(UC_HOOK_CODE, stub_hook, begin=min(STUBS), end=max(STUBS))
call(0x436230, [])
index = struct.unpack("<64i", mu.mem_read(0x6B3A18, 256))
words = struct.unpack(f"<{index[63]}h", mu.mem_read(0x65BBD8, index[63] * 2))
exe_rings = [" ".join(map(str, index)), " ".join(map(str, words))]
report("range rings (0x436230)", 2, sum(a != b for a, b in zip(exe_rings, sh("rings").split("\n"))))

# 6. aircraft launch: the lockstep RNG slot allocator (0x43a9b0) and RNG (0x437b10) are stubbed, bookkeeping increments run
AIR = STACK + 0xA0000
STUB_RET = {0x43A9B0: 0x14, 0x437B10: 0}  # `ret`/`ret imm` pops of the stubbed callees (cdecl: caller cleans the stack)


def cdecl_stub(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)
    uc.reg_write(UC_X86_REG_EAX, 0)


for a in STUB_RET:
    mu.hook_add(UC_HOOK_CODE, cdecl_stub, begin=a, end=a)
OFFS = [4, 8, 0xC, 0x10, 0x14, 0x24, 0x28, 0x2C, 0x34, 0x38, 0x40, 0x44, 0x48, 0x4C, 0x70, 0x74, 0x78, 0x7C, 0x80, 0x88]


def exe_launch(code, tx, ty, side, cells, counter):
    mu.mem_write(AIR, b"\0" * 0x200)
    mu.mem_write(0x71CEC0, struct.pack("<I", cells))
    mu.mem_write(0x89CF50, struct.pack("<I", counter))
    mu.mem_write(0x575730, struct.pack("<I", 99))
    call(0x467EE0, [code, tx, ty, side], ecx=AIR)
    vals = [struct.unpack("<i", mu.mem_read(AIR + o, 4))[0] for o in OFFS]
    vals[6] = code
    return " ".join(map(str, vals))


bad = 0
for _ in range(300):
    code, side = random.choice([30, 31, 32, 33, 34]), random.randint(0, 2)
    cells = random.choice([32, 48, 64, 96, 128])
    tx, ty = random.randint(0, cells * 32), random.randint(0, cells * 32)
    counter = random.randint(0, 9)
    exe = exe_launch(code, tx, ty, side, cells, counter)
    if code == 31:  # the exe stores the incremented counter in [+0x88]
        pass
    bad += exe != sh("launch", code, tx, ty, side, cells, counter)
report("aircraft launch (0x467ee0)", 300, bad)

# 7. carryall squads
bad = 0
calls = []


def unit_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    a = struct.unpack("<6i", uc.mem_read(sp, 24))
    calls.append((a[1], a[2], a[4]))
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, a[0] & 0xFFFFFFFF)


def cookie_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, unit_hook, begin=0x43A420, end=0x43A420)
mu.hook_add(UC_HOOK_CODE, cookie_hook, begin=0x47D8D7, end=0x47D8D7)
for idx in range(18):
    calls.clear()
    mu.mem_write(0x49A33C, struct.pack("<I", 0))
    mu.mem_write(AIR, b"\0" * 0x200)
    mu.mem_write(AIR + 0x88, struct.pack("<I", idx))
    call(0x468960, [10, 20], ecx=AIR)
    exe = "\n".join(f"{x - 10} {y - 20} {k}" for x, y, k in calls)
    bad += exe != sh("squad", idx)
report("carryall squads (0x468960)", 18, bad)

# 8. wave pathfinder: CArray growth/removal and the free call are hooked; the grid, the base and the unit object are set up in memory
import tempfile, os

HEAP, GRID, BASEADDR, UNIT = 0x61000000, 0x5FAC38, 0x49A364, STACK + 0xB0000
# the two layers of the exe (docs/pathfinding.md): ground grid + counter, and the walkers' grid + counter (unit [type+0x4b] != 0)
LAYER_WAVE = [(0x5FAC38, 0x49A364), (0x60AC38, 0x49A368)]
mu.mem_map(HEAP, 0x400000)
heap_top = [HEAP]


def carray_set_size(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_SP if False else UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    n, _grow = struct.unpack("<ii", uc.mem_read(sp + 4, 8))
    arr = uc.reg_read(UC_X86_REG_ECX)
    ptr, cur, cap = struct.unpack("<III", uc.mem_read(arr + 4, 12))
    if n > cap:
        new = max(n * 2, 16)
        newptr = heap_top[0]
        heap_top[0] += new * 8
        if cur:
            uc.mem_write(newptr, bytes(uc.mem_read(ptr, cur * 8)))
        ptr, cap = newptr, new
    uc.mem_write(arr + 4, struct.pack("<III", ptr, n, cap))
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def carray_remove_at(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    idx, count = struct.unpack("<ii", uc.mem_read(sp + 4, 8))
    arr = uc.reg_read(UC_X86_REG_ECX)
    ptr, cur, cap = struct.unpack("<III", uc.mem_read(arr + 4, 12))
    tail = bytes(uc.mem_read(ptr + (idx + count) * 8, (cur - idx - count) * 8))
    uc.mem_write(ptr + idx * 8, tail)
    uc.mem_write(arr + 8, struct.pack("<I", cur - count))
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, carray_set_size, begin=0x45ADF0, end=0x45ADF0)
mu.hook_add(UC_HOOK_CODE, carray_remove_at, begin=0x459770, end=0x459770)
mu.hook_add(UC_HOOK_CODE, cdecl_stub, begin=0x47D321, end=0x47D321)


def exe_wave(grid, base, start, target, drop, mode=0):
    mu.mem_write(LAYER_WAVE[mode][0], struct.pack("<16384I", *[0xFFFFFD if c else 0 for c in grid]))
    mu.mem_write(LAYER_WAVE[mode][1], struct.pack("<I", base))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", start[0] * 32 + 16, start[1] * 32 + 16))
    heap_top[0] = HEAP
    ok = call(0x4753A0, [target[0] * 32 + 16, target[1] * 32 + 16, drop, mode], ecx=UNIT) & 0xFF
    (newbase,) = struct.unpack("<I", mu.mem_read(LAYER_WAVE[mode][1], 4))
    ptr, size = struct.unpack("<II", mu.mem_read(UNIT + 0x88, 8))
    cells = struct.unpack(f"<{size * 2}i", mu.mem_read(ptr, size * 8)) if ok and size else ()
    return f"{'ok' if ok else 'fail'} {newbase} " + " ".join(map(str, cells))


bad = 0
oks = longest = 0
SEARCHES = 300
for n in range(SEARCHES):
    density = random.choice([0.0, 0.1, 0.25, 0.35])
    grid = [0] * 16384
    for x in range(128):
        for y in range(128):
            edge = x < 2 or y < 2 or x > 125 or y > 125  # the exe does no bounds checks: keep the wave off the array edges
            grid[x * 128 + y] = 1 if edge or random.random() < density else 0
    free = [(x, y) for x in range(3, 125) for y in range(3, 125)]
    start, target = random.choice(free), random.choice(free)
    if random.random() < 0.3:  # nearby targets
        target = (min(124, max(3, start[0] + random.randint(-6, 6))), min(124, max(3, start[1] + random.randint(-6, 6))))
    base = random.choice([1, 1, 500, 70000])
    drop = random.choice([0, 0, 1, 2])
    exe = exe_wave(grid, base, start, target, drop, n % 2)  # odd searches run on the walkers' layer (0x474b30)
    with tempfile.NamedTemporaryFile("w", delete=False) as f:
        f.write("\n".join("".join(map(str, grid[x * 128 : x * 128 + 128])) for x in range(128)))
    rust = sh("wave", f.name, *start, *target, drop, base)
    os.unlink(f.name)
    oks += exe.startswith("ok")
    longest = max(longest, len(exe.split()) // 2 - 1)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  wave mismatch", start, target, base, drop, "\n   exe ", exe[:160], "\n   rust", rust[:160])
print(f"  ({oks} routes found, {SEARCHES - oks} failures, longest route {longest} cells)")
report("wave pathfinder (0x4753a0 ground, 0x474b30 walkers; alternating)", SEARCHES, bad)

# 9. walking pixel line (0x46ee80): the route array lives at this+0x98 (same CArray hooks as the wave)
bad = 0
for _ in range(300):
    x0, y0 = random.randint(0, 4000), random.randint(0, 4000)
    x1, y1 = x0 + random.randint(-300, 300), y0 + random.randint(-300, 300)
    if random.random() < 0.15:
        x1 = x0 + random.choice([0, 5, -5])
        y1 = y0 + random.choice([0, 5, -5])
    mu.mem_write(UNIT, b"\0" * 0x200)
    heap_top[0] = HEAP
    call(0x46EE80, [x0, y0, x1, y1], ecx=UNIT)
    ptr, size = struct.unpack("<II", mu.mem_read(UNIT + 0x9C, 8))
    exe = " ".join(map(str, struct.unpack(f"<{size * 2}i", mu.mem_read(ptr, size * 8))))
    bad += exe.split() != sh("line", x0, y0, x1, y1).split()
report("walking pixel line (0x46ee80)", 300, bad)

# 10. local avoidance (0x476870): unit grid 0x61ac38 (base [0x49a36c]), static grid 0x5eac38 (base [0x49a360])
LAYER_LOCAL = [(0x61AC38, 0x5EAC38, 0x49A36C, 0x49A360), (0x62AC38, 0x60AC38, 0x49A374, 0x49A368)]  # unit grid, static grid, their counters
UGRID, SGRID, UBASE, SBASE = LAYER_LOCAL[0]
POS, ROUTE = STACK + 0xB8000, STACK + 0xBC000
bad = 0
stats = {"step": 0, "stop": 0, "none": 0}
for n in range(300):
    UGRID, SGRID, UBASE, SBASE = LAYER_LOCAL[n % 2]  # odd cases go through the dispatcher into 0x473f10
    ugrid = [0] * 16384
    sgrid = [0] * 16384
    for x in range(128):
        for y in range(128):
            if x < 2 or y < 2 or x > 125 or y > 125 or random.random() < random.choice([0.0, 0.05, 0.15]):
                ugrid[x * 128 + y] = sgrid[x * 128 + y] = 0xFFFFFD
    for b in range(random.randint(0, 8)):  # buildings: one handle over a rectangle, in both grids
        bx, by, bw, bh = random.randint(5, 110), random.randint(5, 110), random.randint(1, 5), random.randint(1, 4)
        for x in range(bx, bx + bw):
            for y in range(by, by + bh):
                ugrid[x * 128 + y] = sgrid[x * 128 + y] = 0x2000000 + b
    for u in range(random.randint(0, 30)):  # other units: only in the unit grid
        ugrid[random.randint(5, 120) * 128 + random.randint(5, 120)] = 0x3000000 + u
    at = (random.randint(4, 120), random.randint(4, 120))
    own = 0x3100000 + n
    ugrid[at[0] * 128 + at[1]] = own
    sgrid[at[0] * 128 + at[1]] = 0
    size = random.randint(1, 12)
    k = min(size - 1, 6)
    route = [at] + [(at[0] + random.randint(-8, 8), at[1] + random.randint(-8, 8)) for _ in range(size - 1)]
    route = [(min(124, max(3, x)), min(124, max(3, y))) for x, y in route]
    route[0] = at
    ub, sb = random.choice([1, 1, 500, 70000]), random.choice([1, 40, 70000])
    if random.random() < 0.25 and route[min(size - 1, 6)] != at:  # something stands on the look-ahead cell
        lx, ly = route[min(size - 1, 6)]
        ugrid[lx * 128 + ly] = random.choice([0x3200000, 0xFFFFFD])
        if random.random() < 0.3:
            sgrid[lx * 128 + ly] = random.choice([own, 0x2100000, 0x1ffffe])
    sgrid = [v if v >= 0x1fffff else (0 if random.random() < 0.9 else sb - 1) for v in sgrid]  # static stamps below its base count as free
    mu.mem_write(UGRID, struct.pack("<16384I", *ugrid))
    mu.mem_write(SGRID, struct.pack("<16384I", *sgrid))
    mu.mem_write(UBASE, struct.pack("<I", ub))
    mu.mem_write(SBASE, struct.pack("<I", sb))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(POS, b"\0" * 0x40)
    mu.mem_write(ROUTE, struct.pack(f"<{size * 2}i", *[c for p in route for c in p]))
    mu.mem_write(UNIT + 4, struct.pack("<I", POS))
    mu.mem_write(UNIT + 0x14, struct.pack("<I", own))
    mu.mem_write(UNIT + 0x88, struct.pack("<II", ROUTE, size))
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", -1, -1))
    mu.mem_write(POS + 0x14, struct.pack("<ii", at[0] * 32 + 16, at[1] * 32 + 16))
    heap_top[0] = HEAP
    ok = call(0x476870, [at[0] * 32 + 16, at[1] * 32 + 16, n % 2], ecx=UNIT) & 0xFF
    nx, ny = struct.unpack("<ii", mu.mem_read(UNIT + 0x6C, 8))
    gx, gy = struct.unpack("<ii", mu.mem_read(UNIT + 0x5C, 8))
    (nb,) = struct.unpack("<I", mu.mem_read(UBASE, 4))
    ptr, psize = struct.unpack("<II", mu.mem_read(UNIT + 0xB0, 8))
    path = struct.unpack(f"<{psize * 2}i", mu.mem_read(ptr, psize * 8)) if psize else ()
    stop = int(gx != -1)
    if ok:
        exe = f"0 {nb} | {nx} {ny} | " + " ".join(map(str, path))
        stats["step"] += 1
    else:
        exe = f"{stop} {nb} | none | "
        stats["stop" if stop else "none"] += 1
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<32768I", *(ugrid + sgrid)))
    rust = sh("local", f.name, ub, sb, own, *at, *[c for p in route for c in p])
    os.unlink(f.name)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  local mismatch at", at, "route", route[:k + 1][-1], "bases", ub, sb, "\n   exe ", exe[:150], "\n   rust", rust[:150])
print(f"  ({stats['step']} steps, {stats['stop']} stops, {stats['none']} without a step)")
report("local avoidance (0x476870 ground, 0x473f10 walkers; alternating)", 300, bad)
UGRID, SGRID, UBASE, SBASE = LAYER_LOCAL[0]

# 10b. second-step local search (0x475c30 ground, 0x4732f0 via its mode argument): the same search as 0x476870 over the second step
# cell grid, plus a free-check of the first step grid when the look-ahead cell is next to the unit (called from 0x472f50, networked games only)
LAYER_LOCAL2 = [(0x63AC38, 0x5EAC38, 0x61AC38, 0x49A370, 0x49A360, 0x49A36C), (0x64AC38, 0x60AC38, 0x62AC38, 0x49A378, 0x49A368, 0x49A374)]
bad = 0
stats = {"step": 0, "none": 0, "adjacent": 0, "stops": 0}
for n in range(300):
    UGRID, SGRID, FGRID, UBASE, SBASE, FBASE = LAYER_LOCAL2[n % 2]
    ugrid, sgrid, fgrid = [0] * 16384, [0] * 16384, [0] * 16384
    for x in range(128):
        for y in range(128):
            if x < 2 or y < 2 or x > 125 or y > 125 or random.random() < random.choice([0.0, 0.05, 0.15]):
                ugrid[x * 128 + y] = sgrid[x * 128 + y] = fgrid[x * 128 + y] = 0xFFFFFD
    for b in range(random.randint(0, 8)):
        bx, by, bw, bh = random.randint(5, 110), random.randint(5, 110), random.randint(1, 5), random.randint(1, 4)
        for x in range(bx, bx + bw):
            for y in range(by, by + bh):
                ugrid[x * 128 + y] = sgrid[x * 128 + y] = fgrid[x * 128 + y] = 0x2000000 + b
    for u in range(random.randint(0, 30)):  # other units: reserved cells in the second grid and (other cells) in the first
        ugrid[random.randint(5, 120) * 128 + random.randint(5, 120)] = 0x3000000 + u
    for u in range(random.randint(0, 30)):
        fgrid[random.randint(5, 120) * 128 + random.randint(5, 120)] = 0x3400000 + u
    at = (random.randint(4, 120), random.randint(4, 120))
    own = 0x3100000 + n
    ugrid[at[0] * 128 + at[1]] = own
    sgrid[at[0] * 128 + at[1]] = 0
    size = random.randint(1, 12)
    k = min(size - 1, 6)
    route = [at] + [(at[0] + random.randint(-8, 8), at[1] + random.randint(-8, 8)) for _ in range(size - 1)]
    route = [(min(124, max(3, x)), min(124, max(3, y))) for x, y in route]
    route[0] = at
    if random.random() < 0.3 and size > 1:  # look-ahead cell right next to the unit: the free test of both grids
        route[k] = (at[0] + random.randint(-1, 1), at[1] + random.randint(-1, 1))
    ub, sb, fb = random.choice([1, 1, 500, 70000]), random.choice([1, 40, 70000]), random.choice([1, 1, 300, 70000])
    if random.random() < 0.25 and route[k] != at:
        lx, ly = route[k]
        ugrid[lx * 128 + ly] = random.choice([0x3200000, 0xFFFFFD, 0])
        fgrid[lx * 128 + ly] = random.choice([0x3200000, 0xFFFFFD, 0, 0])
        if random.random() < 0.3:
            sgrid[lx * 128 + ly] = random.choice([own, 0x2100000, 0x1ffffe])
    sgrid = [v if v >= 0x1fffff else (0 if random.random() < 0.9 else sb - 1) for v in sgrid]
    fgrid = [v if v >= 0x1fffff else (0 if random.random() < 0.9 else fb - 1) for v in fgrid]
    mu.mem_write(UGRID, struct.pack("<16384I", *ugrid))
    mu.mem_write(SGRID, struct.pack("<16384I", *sgrid))
    mu.mem_write(FGRID, struct.pack("<16384I", *fgrid))
    mu.mem_write(UBASE, struct.pack("<I", ub))
    mu.mem_write(SBASE, struct.pack("<I", sb))
    mu.mem_write(FBASE, struct.pack("<I", fb))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(POS, b"\0" * 0x40)
    mu.mem_write(ROUTE, struct.pack(f"<{size * 2}i", *[c for p in route for c in p]))
    mu.mem_write(UNIT + 4, struct.pack("<I", POS))
    mu.mem_write(UNIT + 0x14, struct.pack("<I", own))
    mu.mem_write(UNIT + 0x88, struct.pack("<II", ROUTE, size))
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", -1, -1))
    mu.mem_write(UNIT + 0x6C, struct.pack("<ii", -1, -1))
    mu.mem_write(UNIT + 0x74, struct.pack("<ii", -1, -1))
    heap_top[0] = HEAP
    ok = call(0x475C30, [at[0] * 32 + 16, at[1] * 32 + 16, n % 2], ecx=UNIT) & 0xFF
    nx, ny = struct.unpack("<ii", mu.mem_read(UNIT + 0x74, 8))
    first = struct.unpack("<ii", mu.mem_read(UNIT + 0x6C, 8))
    gx, gy = struct.unpack("<ii", mu.mem_read(UNIT + 0x5C, 8))
    (nb,) = struct.unpack("<I", mu.mem_read(UBASE, 4))
    ptr, psize = struct.unpack("<II", mu.mem_read(UNIT + 0xB0, 8))
    path = struct.unpack(f"<{psize * 2}i", mu.mem_read(ptr, psize * 8)) if psize else ()
    stop = int(gx != -1)  # only the air routine 0x4732f0 resets the destination
    if first != (-1, -1) or stop and n % 2 == 0:
        print("  0x475c30 wrote outside the second step cell:", first, (gx, gy), "ok", ok, "at", at, "route", route)
        bad += 1
    if ok:
        exe = f"0 {nb} | {nx} {ny} | " + " ".join(map(str, path))
        stats["step"] += 1
        stats["adjacent"] += not path
    else:
        exe = f"{stop} {nb} | none | "
        stats["stops"] += stop
        stats["none"] += 1
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<49152I", *(ugrid + sgrid + fgrid)))
    rust = sh("local2", f.name, ub, sb, fb, "air" if n % 2 else "ground", own, *at, *[c for p in route for c in p])
    os.unlink(f.name)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  local2 mismatch at", at, "route", route[:k + 1][-1], "bases", ub, sb, fb, "\n   exe ", exe[:150], "\n   rust", rust[:150])
print(f"  ({stats['step']} steps of which {stats['adjacent']} next to the unit, {stats['none']} without a step, {stats['stops']} stops of the air routine)")
report("second-step local search (0x475c30 ground, 0x4732f0 walkers; alternating)", 300, bad)
UGRID, SGRID, UBASE, SBASE = LAYER_LOCAL[0]

# 10c. free cell near the destination (0x4774c0, the fallback of the planners): ground = flood of the unit grid from the unit's cell
# (0x4726f0) + ring scan for the own handle or the region id; air = ring scan for the own handle or a value below 0x11170
bad = 0
stats = {"moved": 0, "kept": 0, "none": 0, "air": 0}
for n in range(300):
    air = n % 2
    UGRID, UBASE = (0x62AC38, 0x49A374) if air else (0x61AC38, 0x49A36C)
    grid = [0] * 16384
    dens = random.choice([0.0, 0.1, 0.3, 0.5])
    for x in range(128):
        for y in range(128):
            if x < 2 or y < 2 or x > 125 or y > 125 or random.random() < dens:
                grid[x * 128 + y] = 0xFFFFFD
    for b in range(random.randint(0, 10)):
        bx, by, bw, bh = random.randint(5, 110), random.randint(5, 110), random.randint(1, 12), random.randint(1, 12)
        for x in range(bx, bx + bw):
            for y in range(by, by + bh):
                grid[x * 128 + y] = 0x2000000 + b
    if random.random() < 0.3:  # a wall splitting the room (the ground flood must not cross it)
        wx = random.randint(30, 90)
        for y in range(128):
            grid[wx * 128 + y] = 0xFFFFFD
    for u in range(random.randint(0, 40)):
        grid[random.randint(5, 120) * 128 + random.randint(5, 120)] = 0x3000000 + u
    ub = random.choice([1, 500, 70000, 1900000])
    for _ in range(random.randint(0, 200)):  # old stamps of earlier searches (below the base: free for the flood; small for the air scan)
        grid[random.randint(5, 120) * 128 + random.randint(5, 120)] = random.choice([0, 3, 0x5000, 0x11170, 0x20000]) % max(ub, 1) if not air else random.choice([0x5000, 0x11170, 0x11171, 0x20000])
    own = 0x3100000 + n
    dest = (random.randint(36, 90) * 32 + random.randint(0, 31), random.randint(36, 90) * 32 + random.randint(0, 31))
    at = (random.randint(5, 120) * 32 + random.randint(0, 31), random.randint(5, 120) * 32 + random.randint(0, 31))
    if random.random() < 0.5:
        grid[(dest[0] >> 5) * 128 + (dest[1] >> 5)] = random.choice([own, 0x3000099])
    if random.random() < 0.3:  # the unit's own handle somewhere around the destination
        grid[((dest[0] >> 5) + random.randint(-6, 6)) * 128 + (dest[1] >> 5) + random.randint(-6, 6)] = own
    mapw, maph = random.choice([128, 64, 64, 50, 40]), random.choice([128, 64, 64, 50, 40])
    mu.mem_write(UGRID, struct.pack("<16384I", *grid))
    mu.mem_write(UBASE, struct.pack("<I", ub))
    mu.mem_write(0x71CEC0, struct.pack("<II", mapw, maph))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(POS, b"\0" * 0x80)
    mu.mem_write(POS + 0x4B, bytes([air]))
    mu.mem_write(UNIT + 4, struct.pack("<I", POS))
    mu.mem_write(UNIT + 0x14, struct.pack("<I", own))
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", *dest))
    heap_top[0] = HEAP
    ok = call(0x4774C0, [at[0], at[1]], ecx=UNIT) & 0xFF
    nd = struct.unpack("<ii", mu.mem_read(UNIT + 0x5C, 8))
    (nb,) = struct.unpack("<I", mu.mem_read(UBASE, 4))
    g2 = struct.unpack("<16384I", mu.mem_read(UGRID, 65536))
    csum = sum((i + 1) * v for i, v in enumerate(g2)) & 0xFFFFFFFF
    moved = nd != dest
    exe = f"{ok} {nb} {nd[0]} {nd[1]} {csum}"  # the exe may also write the cell centre the destination already was
    stats["air" if air else "moved"] += 0
    stats["moved" if moved else ("kept" if ok else "none")] += 1
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<16384I", *grid))
    rust = sh("freecell", f.name, ub, own, *dest, *at, "air" if air else "ground", mapw, maph)
    os.unlink(f.name)
    rust = rust.replace("none", f"{dest[0]} {dest[1]}")  # `none` = the destination stays as it was
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  freecell mismatch", n, "dest", dest, "at", at, "\n   exe ", exe, "\n   rust", rust)
print(f"  ({stats['moved']} destinations moved, {stats['kept']} kept by the map test or the same cell, {stats['none']} without a cell)")
report("free cell near the destination (0x4774c0 ground with flood 0x4726f0, air; alternating)", 300, bad)

# 11. chase cell search (0x477630): the handle queries are hooked (a live unit target at a given pixel position)
TOBJ, OUT = STACK + 0xBA000, STACK + 0xBB000


def make_ret_hook(value):
    def hook(uc, addr, size, ud):
        sp = uc.reg_read(UC_X86_REG_ESP)
        (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_EAX, value)
    return hook


HTYPE = [0]


def handle_type_hook(uc, addr, size, ud):
    make_ret_hook(HTYPE[0])(uc, addr, size, ud)


chase_hooks = [mu.hook_add(UC_HOOK_CODE, handle_type_hook, begin=0x43AAA0, end=0x43AAA0)]  # handle type: 4 sends the chase to 0x478200
chase_hooks.append(mu.hook_add(UC_HOOK_CODE, make_ret_hook(1), begin=0x43AAB0, end=0x43AAB0))  # a unit
chase_hooks.append(mu.hook_add(UC_HOOK_CODE, make_ret_hook(TOBJ), begin=0x43AAC0, end=0x43AAC0))  # alive: its object
bad = 0
stats = {"ok": 0, "fail": 0}
for n in range(300):
    HTYPE[0] = 4 * (n % 2)  # odd cases: the target's handle type is 4 -> 0x478200 on the walkers' grid
    SGRID, SBASE = LAYER_LOCAL[n % 2][1], LAYER_LOCAL[n % 2][3]
    mapw, maph = random.choice([(64, 64), (48, 40), (100, 100)])
    grid = [0] * 16384
    for x in range(128):
        for y in range(128):
            if x < 1 or y < 1 or x >= mapw - 1 or y >= maph - 1 or random.random() < random.choice([0.0, 0.1, 0.25, 0.35]):
                grid[x * 128 + y] = 0xFFFFFD
    free = [(x, y) for x in range(4, mapw - 4) for y in range(4, maph - 4)]
    unit, target = random.choice(free), random.choice(free)
    handle = 0x2000007
    for _ in range(random.randint(0, 6)):  # cells of the target's own building around it
        grid[(target[0] + random.randint(-2, 2)) * 128 + target[1] + random.randint(-2, 2)] = handle
    if random.random() < 0.2:  # wall the target in: the wave never gets near it
        for dx in range(-3, 4):
            for dy in range(-3, 4):
                if max(abs(dx), abs(dy)) == 3:
                    grid[(target[0] + dx) * 128 + target[1] + dy] = 0xFFFFFD
    grid[unit[0] * 128 + unit[1]] = random.choice([0, 0, 0x3000000])
    base = random.choice([1, 1, 500, 70000])
    rng_ = random.choice([1, 2, 3, 5, 8, 12])
    mu.mem_write(SGRID, struct.pack("<16384I", *grid))
    mu.mem_write(SBASE, struct.pack("<I", base))
    mu.mem_write(0x71CEC0, struct.pack("<II", mapw, maph))
    mu.mem_write(TOBJ, b"\0" * 0x40)
    mu.mem_write(TOBJ + 0x14, struct.pack("<ii", target[0] * 32 + 16, target[1] * 32 + 16))
    mu.mem_write(OUT, b"\0" * 16)
    ok = call(0x477630, [unit[0], unit[1], handle, rng_, OUT, OUT + 4], ecx=UNIT) & 0xFF
    ox, oy = struct.unpack("<ii", mu.mem_read(OUT, 8))
    (nb,) = struct.unpack("<I", mu.mem_read(SBASE, 4))
    exe = f"ok {ox} {oy} {nb}" if ok else f"fail {nb}"
    stats["ok" if ok else "fail"] += 1
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<16384I", *grid))
    rust = sh("approach", f.name, base, mapw, maph, *unit, *target, handle, rng_, *(["air"] if n % 2 else []))
    os.unlink(f.name)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  approach mismatch", unit, target, "range", rng_, "base", base, "\n   exe ", exe, "\n   rust", rust)
print(f"  ({stats['ok']} destinations, {stats['fail']} failures)")
report("chase cell search (0x477630 ground, 0x478200 type 4; alternating)", 300, bad)
HTYPE[0] = 0
SGRID, SBASE = LAYER_LOCAL[0][1], LAYER_LOCAL[0][3]

for h in chase_hooks:  # the handle decoders run as real code in the next test
    mu.hook_del(h)

# 0x472a90  target reachability scan (docs/ai.md) vs `opensow targets`
def make_handle(typ, owner, sub, index):
    return (typ << 22) | (sub << 19) | (owner << 16) | index


bad = 0
found_any = 0
for n in range(300):
    mapw, maph = random.choice([(64, 64), (48, 40), (100, 100)])
    grid = [0] * 16384
    for x in range(128):
        for y in range(128):
            if x < 1 or y < 1 or x >= mapw - 1 or y >= maph - 1 or random.random() < random.choice([0.0, 0.1, 0.25, 0.35]):
                grid[x * 128 + y] = 0xFFFFFD
    free = [(x, y) for x in range(4, mapw - 4) for y in range(4, maph - 4)]
    start = random.choice(free)
    for k in range(random.randint(0, 25)):
        cx, cy = random.choice(free)
        typ = random.choice([1, 1, 2, 2, 2, 3])
        grid[cx * 128 + cy] = make_handle(typ, random.randint(0, 2), random.choice([0, 1, 3, 6, 7]), k + 1)
    own = random.choice([0, 1])
    base = random.choice([1, 1, 500, 70000])
    mu.mem_write(SGRID, struct.pack("<16384I", *grid))
    mu.mem_write(SBASE, struct.pack("<I", base))
    mu.mem_write(0x71CEC0, struct.pack("<II", mapw, maph))
    mu.mem_write(OUT, b"\0" * 16)
    call(0x472A90, [start[0] * 32 + 7, start[1] * 32 + 9, own, OUT, OUT + 4, OUT + 8])
    unit_h, bld_h, tur_h = struct.unpack("<III", mu.mem_read(OUT, 12))
    (nb,) = struct.unpack("<I", mu.mem_read(SBASE, 4))
    exe = f"{unit_h} {bld_h} {tur_h} {nb}"
    found_any += (unit_h or bld_h or tur_h) != 0
    # the grid must come back unchanged apart from stamps: compare the start cell
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<16384I", *grid))
    rust = sh("targets", f.name, base, mapw, maph, *start, own)
    os.unlink(f.name)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  targets mismatch", start, "own", own, "base", base, "\n   exe ", exe, "\n   rust", rust)
print(f"  ({found_any} scans found something)")
report("target reachability scan (0x472a90)", 300, bad)

# 12. networked chassis step (0x46fa40, docs/pathfinding.md): position from the game clock. Its callees are stubbed and recorded:
# planner 0x472f50 (writes the step cells and a line), chase check 0x46eb40, wave 0x4753a0, order removal 0x470810, next order
# 0x470480, line builder 0x46ee80. The facing 0x46b180 and the local-player test 0x41f020 run for real; fs:[0] (SEH frame) is mapped.
try:
    mu.mem_map(0, 0x1000)
except Exception:
    pass
LINEA, LINEB, POS12 = HEAP + 0x100000, HEAP + 0x110000, STACK + 0xC8000
calls12, cfg12 = [], {}


def ret_stub(pops, fn):
    def hook(uc, addr, size, ud):
        sp = uc.reg_read(UC_X86_REG_ESP)
        (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
        args = struct.unpack(f"<{pops // 4}i", uc.mem_read(sp + 4, pops)) if pops else ()
        eax = fn(uc, args)
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_EAX, eax or 0)

    return hook


def set_line12(uc, pts):
    uc.mem_write(LINEB, struct.pack(f"<{len(pts) * 2}i", *[c for p in pts for c in p]) if pts else b"")
    uc.mem_write(UNIT + 0x9C, struct.pack("<III", LINEB if pts else 0, len(pts), len(pts)))


def st_plan(uc, a):
    calls12.append(f"plan({a[0]},{a[1]})")
    s1, s2, pl = cfg12["plan"]
    uc.mem_write(UNIT + 0x6C, struct.pack("<ii", *s1))
    uc.mem_write(UNIT + 0x74, struct.pack("<ii", *s2))
    set_line12(uc, pl)


def st_route(uc, a):
    calls12.append(f"route({a[0]},{a[1]})")
    if not cfg12["route"]:
        calls12.append("clear")  # the exe frees the order list inline after a failed search
    return int(cfg12["route"])


def st_line(uc, a):
    calls12.append(f"line({a[0]},{a[1]},{a[2]},{a[3]})")
    set_line12(uc, [(a[0], a[1]), (a[2], a[3])])


def st_pop(uc, a):
    calls12.append("pop" if a == (0, 1) else f"pop{a}")


mu.hook_add(UC_HOOK_CODE, ret_stub(8, st_plan), begin=0x472F50, end=0x472F50)
mu.hook_add(UC_HOOK_CODE, ret_stub(8, lambda uc, a: calls12.append(f"chase({a[0]},{a[1]})")), begin=0x46EB40, end=0x46EB40)
mu.hook_add(UC_HOOK_CODE, ret_stub(16, st_route), begin=0x4753A0, end=0x4753A0)
mu.hook_add(UC_HOOK_CODE, ret_stub(8, st_pop), begin=0x470810, end=0x470810)
mu.hook_add(UC_HOOK_CODE, ret_stub(0, lambda uc, a: calls12.append("start")), begin=0x470480, end=0x470480)
mu.hook_add(UC_HOOK_CODE, ret_stub(16, st_line), begin=0x46EE80, end=0x46EE80)

bad = 0
stats = {"along": 0, "end": 0, "arrive": 0, "fail": 0, "guest": 0, "plan": 0}
for n in range(300):
    air = n % 2
    guest = random.random() < 0.25
    SG = 0x60AC38 if air else 0x5EAC38
    cell = lambda: random.randint(3, 120)
    px = lambda: random.randint(100, 3900)
    npts = random.choice([0, 1, 2, 3, 5, 8])
    line = [(px(), px()) for _ in range(npts)]
    pos = line[0] if line and random.random() < 0.8 else (px(), px())
    since = (random.randint(0, 5), random.randint(0, 59))
    adv = random.choice([0, 1, 1, 2, 3, 5, 9, 70])
    t = since[1] + adv
    now = (since[0] + t // 60, t % 60) if random.random() > 0.05 else (since[0] - 1, since[1])
    row_idx = random.randint(0, 30)
    row = [random.choice([0, 1, 1, 2, 3]) for _ in range(60)]
    dest = random.choice([pos, (px(), px()), line[-1] if line else pos])
    step1, step2, prev = (cell(), cell()), (cell(), cell()), (cell(), cell())
    plan = ((cell(), cell()), (cell(), cell()), [(px(), px()) for _ in range(random.choice([0, 2, 4]))])
    cd = random.choice([-1, 0, 0, 3, 6])
    anim = (random.randint(-1, 2), random.randint(0, 5), random.randint(0, 3), random.choice([1, 4, 4]))
    kind40 = random.random() < 0.3
    cfg12.update(plan=plan, route=random.random() < 0.7)
    handle = 0x3100000 + n
    stat = [0] * 16384
    for _ in range(80):
        stat[random.randint(0, 16383)] = random.choice([0x3000001, 0xFFFFFD])
    mu.mem_write(SG, struct.pack("<16384I", *stat))
    mu.mem_write(0x5524A0 + row_idx * 60, bytes(row))
    mu.mem_write(0x575720, struct.pack("<I", 1))
    mu.mem_write(0x575730, struct.pack("<I", 1 if guest else 0))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(POS12, b"\0" * 0x80)
    mu.mem_write(UNIT + 4, struct.pack("<I", POS12))
    mu.mem_write(POS12 + 0x4B, bytes([air]))
    mu.mem_write(POS12 + 0x40, struct.pack("<i", 40 if kind40 else 5))
    mu.mem_write(POS12 + 0x14, struct.pack("<ii", *pos))
    mu.mem_write(UNIT + 8, struct.pack("<i", 32))
    mu.mem_write(UNIT + 0x14, struct.pack("<I", handle))
    mu.mem_write(UNIT + 0x18, struct.pack("<i", row_idx))
    mu.mem_write(UNIT + 0x44, struct.pack("<ii", anim[2], anim[3]))
    mu.mem_write(UNIT + 0x4C, struct.pack("<ii", anim[0], anim[1]))
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", *dest))
    mu.mem_write(UNIT + 0x64, struct.pack("<ii", *prev))
    mu.mem_write(UNIT + 0x6C, struct.pack("<ii", *step1))
    mu.mem_write(UNIT + 0x74, struct.pack("<ii", *step2))
    mu.mem_write(UNIT + 0x80, struct.pack("<i", cd))
    mu.mem_write(UNIT + 0x88, struct.pack("<III", 0x61200000, 7, 7))
    mu.mem_write(UNIT + 0x168, struct.pack("<ii", *since))
    mu.mem_write(LINEA, struct.pack(f"<{npts * 2}i", *[c for p in line for c in p]))
    mu.mem_write(UNIT + 0x9C, struct.pack("<III", LINEA if npts else 0, npts, npts))
    calls12.clear()
    call(0x46FA40, [POS12 + 0x14, POS12 + 0x18, now[0], now[1]], ecx=UNIT)
    rx = lambda off, k=2: struct.unpack(f"<{k}i", mu.mem_read(UNIT + off, 4 * k))
    fpos = struct.unpack("<ii", mu.mem_read(POS12 + 0x14, 8))
    lptr, ln, _ = struct.unpack("<III", mu.mem_read(UNIT + 0x9C, 12))
    fline = struct.unpack(f"<{ln * 2}i", mu.mem_read(lptr, ln * 8)) if ln else ()
    g2 = struct.unpack("<16384I", mu.mem_read(SG, 65536))
    csum = sum((i + 1) * v for i, v in enumerate(g2)) & 0xFFFFFFFF
    fac = rx(0x30, 1)[0]
    exe = (
        f"pos={fpos[0]},{fpos[1]} since={rx(0x168)[0]},{rx(0x168)[1]} line=[{';'.join(f'{fline[i]},{fline[i + 1]}' for i in range(0, len(fline), 2))}] "
        f"prev={rx(0x64)[0]},{rx(0x64)[1]} step1={rx(0x6C)[0]},{rx(0x6C)[1]} step2={rx(0x74)[0]},{rx(0x74)[1]} facing={fac} want={rx(0x34, 1)[0]} "
        f"frame={rx(0x40, 1)[0]} cd={rx(0x80, 1)[0]} anim={rx(0x4C, 1)[0]},{rx(0x44, 1)[0]} routelen={rx(0x8C, 1)[0]} stat={csum} calls={' '.join(calls12)}"
    )
    stats["guest" if guest else "plan"] += any(c.startswith("line") if guest else c.startswith("plan") for c in calls12)
    stats["fail"] += "clear" in calls12
    stats["arrive"] += "pop" in calls12
    stats["along"] += not calls12
    stats["end"] += bool(calls12)
    with tempfile.NamedTemporaryFile("wb", delete=False) as f:
        f.write(struct.pack("<16384I", *stat))
    pl = ";".join(f"{x},{y}" for x, y in plan[2])
    flat = lambda pts: ",".join(f"{c}" for p in pts for c in p)
    rust = sh(
        "netstep", f"air={air}", f"guest={int(guest)}", f"route={int(cfg12['route'])}", f"now={now[0]},{now[1]}", f"since={since[0]},{since[1]}",
        f"pos={pos[0]},{pos[1]}", f"dest={dest[0]},{dest[1]}", f"handle={handle}", "row=" + ",".join(map(str, row)), "line=" + flat(line),
        f"prev={prev[0]},{prev[1]}", f"step1={step1[0]},{step1[1]}", f"step2={step2[0]},{step2[1]}", f"plan1={plan[0][0]},{plan[0][1]}",
        f"plan2={plan[1][0]},{plan[1][1]}", "planline=" + flat(plan[2]), f"cd={cd}", f"anim={anim[0]},{anim[1]},{anim[2]},{anim[3]}",
        f"kind40={int(kind40)}", "turn=32", "routelen=7", f"stat={f.name}",
    )
    os.unlink(f.name)
    if exe.split(" calls=")[0].split() != rust.split(" calls=")[0].split() or exe.split("calls=")[1:] != rust.split("calls=")[1:]:
        bad += 1
        if bad <= 3:
            print("  netstep mismatch", n, "guest", guest, "air", air, "\n   exe ", exe, "\n   rust", rust)
print(f"  ({stats['along']} moves along the line, {stats['end']} line ends of which {stats['arrive']} arrivals, {stats['fail']} failed routes, {stats['plan']} host plans, {stats['guest']} guest lines)")
report("networked step (0x46fa40; callees stubbed, facing and 0x41f020 real)", 300, bad)


sys.exit(1 if failures else 0)
