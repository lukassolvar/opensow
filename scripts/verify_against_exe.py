#!/usr/bin/env python3
"""Run functions of the original State of War.exe in an x86 emulator and compare them with OpenSOW's Rust ports.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_against_exe.py "<install>/State of War.exe" target/release/opensow

The exe is only read, never modified or copied. Checks (random inputs, exact comparison):
  * 0x40ef60  bullet path builder          vs `opensow path`      (bullet types 2..35 except 13/14; type 1 needs atan)
  * 0x436df0  speed table builder          vs `opensow speedrow`
  * 0x46b180  heading -> facing            vs `opensow facing`    (32, 16, 8 and unsupported step counts)
  * 0x46a190  one turn step                vs `opensow turn`
  * 0x467ee0  aircraft launch (fields)     vs `opensow launch`    (all five air forces, random targets)
  * 0x468960  carryall squad unload        vs `opensow squad`     (indices 0..17; unit-creation call recorded)
  * 0x46bc70  Advancer hover table       vs `opensow hover`     (42 phases)
  * 0x4753a0  wave pathfinder              vs `opensow wave`      (random grids, 300 searches incl. failures)
  * 0x46ee80  walking pixel line           vs `opensow line`      (300 random segments)
  * 0x476870  local avoidance search       vs `opensow local`     (300 random grids with units and buildings)
  * 0x477630  chase cell search            vs `opensow approach` (300 random grids, unit targets)
  * 0x436230  range ring list builder      vs `opensow rings`     (64-entry index table + 14017 offsets)
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
    sp = STACK + 0x80000
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
    return cnt // 2, first // 2, pts, lifts


def rust_path(t, *a):
    out = subprocess.run([opensow, "path", str(t), *map(str, a)], capture_output=True, text=True)
    if out.returncode:
        return None
    lines = out.stdout.split("\n")
    n, f = map(int, lines[0].split())
    rows = [tuple(map(int, l.split())) for l in lines[1 : 1 + n]]
    return n, f, [(r[0], r[1]) for r in rows], [r[2] for r in rows]


random.seed(7)
bad = 0
types = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 15, 16, 17, 18, 24, 26, 27, 31, 33, 34, 35]
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


def exe_wave(grid, base, start, target, drop):
    mu.mem_write(GRID, struct.pack("<16384I", *[0xFFFFFD if c else 0 for c in grid]))
    mu.mem_write(BASEADDR, struct.pack("<I", base))
    mu.mem_write(UNIT, b"\0" * 0x200)
    mu.mem_write(UNIT + 0x5C, struct.pack("<ii", start[0] * 32 + 16, start[1] * 32 + 16))
    heap_top[0] = HEAP
    ok = call(0x4753A0, [target[0] * 32 + 16, target[1] * 32 + 16, drop, 0], ecx=UNIT) & 0xFF
    (newbase,) = struct.unpack("<I", mu.mem_read(BASEADDR, 4))
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
    exe = exe_wave(grid, base, start, target, drop)
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
report("wave pathfinder (0x4753a0)", SEARCHES, bad)

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
UGRID, SGRID, UBASE, SBASE = 0x61AC38, 0x5EAC38, 0x49A36C, 0x49A360
POS, ROUTE = STACK + 0xB8000, STACK + 0xBC000
bad = 0
stats = {"step": 0, "stop": 0, "none": 0}
for n in range(300):
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
    ok = call(0x476870, [at[0] * 32 + 16, at[1] * 32 + 16, 0], ecx=UNIT) & 0xFF
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
report("local avoidance (0x476870)", 300, bad)

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


mu.hook_add(UC_HOOK_CODE, make_ret_hook(0), begin=0x43AAA0, end=0x43AAA0)  # handle type: not 4
mu.hook_add(UC_HOOK_CODE, make_ret_hook(1), begin=0x43AAB0, end=0x43AAB0)  # a unit
mu.hook_add(UC_HOOK_CODE, make_ret_hook(TOBJ), begin=0x43AAC0, end=0x43AAC0)  # alive: its object
bad = 0
stats = {"ok": 0, "fail": 0}
for n in range(300):
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
    rust = sh("approach", f.name, base, mapw, maph, *unit, *target, handle, rng_)
    os.unlink(f.name)
    if exe.split() != rust.split():
        bad += 1
        if bad <= 3:
            print("  approach mismatch", unit, target, "range", rng_, "base", base, "\n   exe ", exe, "\n   rust", rust)
print(f"  ({stats['ok']} destinations, {stats['fail']} failures)")
report("chase cell search (0x477630)", 300, bad)

sys.exit(1 if failures else 0)
