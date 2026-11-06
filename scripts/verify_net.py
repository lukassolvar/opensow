#!/usr/bin/env python3
"""Check OpenSOW's multiplayer packet layouts (`formats::net`) against the exe's own bit writers and readers.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_net.py "<install>/State of War.exe" target/release/opensow

  * 0x451540 / 0x451920 (CNetMessage2 key and mouse writers) build random input packages; `opensow netpkg` must decode them.
  * 0x4500d0 / 0x44fe00 (the readers) must give the same values on those packages.
  * 0x4527c0 (header field writer) must set exactly the bits of `net::HEADER_FIELDS`.
  * 0x40a2d0, 0x4128f0, 0x4688d0, 0x46c5c0 (the state-sync writers) run on random objects must decode to the same fields with `opensow netsync` (`formats::netsync`).
"""
import random, struct, subprocess, sys
import pefile
from unicorn import UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path); base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, OBJ, OUT, RET = 0x70000000, 0x60000000, 0x61000000, 0x50000000
mu.mem_map(STACK, 0x100000); mu.mem_map(OBJ, 0x10000); mu.mem_map(OUT, 0x1000); mu.mem_map(RET, 0x1000)
mu.mem_write(RET, b"\xf4")

def call(func, args, this=OBJ):
    sp = STACK + 0x80000
    for a in reversed(args):
        sp -= 4; mu.mem_write(sp, struct.pack("<I", a))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp); mu.reg_write(UC_X86_REG_ECX, this)
    mu.emu_start(func, RET, count=200000)

def u32(a):
    return struct.unpack("<I", mu.mem_read(a, 4))[0]

rnd = random.Random(7)
bad = 0
for case in range(200):
    # fresh CNetMessage2: zeroed, write position 16
    mu.mem_write(OBJ, b"\0" * 0x5000)
    mu.mem_write(OBJ + 0x3F0, struct.pack("<I", 16))
    keys = [(rnd.randrange(256), rnd.choice([1, 3])) for _ in range(rnd.randrange(0, 6))]
    for k, t in keys:
        call(0x451540, [k, t])
    x, y, a, b = rnd.randrange(65536), rnd.randrange(65536), rnd.randrange(2), rnd.randrange(2)
    call(0x451920, [x, y, a, b])
    nbits = u32(OBJ + 0x3F0)
    data = bytes(mu.mem_read(OBJ + 4, (nbits + 7) // 8))
    out = subprocess.check_output([opensow, "netpkg", data.hex(), str(nbits)], text=True).strip()
    sx = x - 65536 if x >= 32768 else x
    sy = y - 65536 if y >= 32768 else y
    want = "keys %s mouse %d %d %d %d" % (",".join("%d:%d" % (k, 1 if t == 1 else 0) for k, t in keys), sx, sy, a, b)
    if out != want:
        bad += 1; print("MISMATCH writer->rust", out, "!=", want)
    # exe readers on the same package
    mu.mem_write(OBJ + 0x3EC, struct.pack("<I", 16))
    for k, t in keys:
        call(0x44FE00, [OUT, OUT + 4])
        if mu.mem_read(OUT, 1)[0] != k or u32(OUT + 4) & 0xFF != t:
            bad += 1; print("MISMATCH key reader", k, t)
    call(0x4500D0, [OUT, OUT + 4, OUT + 8, OUT + 12])
    got = (struct.unpack("<i", mu.mem_read(OUT, 4))[0], struct.unpack("<i", mu.mem_read(OUT + 4, 4))[0])
    if got != (sx, sy):
        bad += 1; print("MISMATCH mouse reader", got, (sx, sy))

# header field writer 0x4527c0(selector, value): buffer at +0xc, write bits at fixed positions
FIELDS = [(0, 4), (7, 18), (0x19, 6), (0x1F, 12), (0x2B, 9), (0x34, 9), (0x3D, 6), (4, 3)]
for sel, (at, n) in enumerate(FIELDS):
    for _ in range(20):
        v = rnd.randrange(1 << n)
        mu.mem_write(OBJ, b"\0" * 0x5000)
        call(0x4527C0, [sel, v])
        buf = bytes(mu.mem_read(OBJ + 0xC, 16))
        bits = [(buf[i >> 3] >> (i & 7)) & 1 for i in range(128)]
        got = sum(bits[at + i] << i for i in range(n))
        others = [i for i, bit in enumerate(bits) if bit and not at <= i < at + n]
        if got != v or others:
            bad += 1; print("MISMATCH header field", sel, v, got, others)
# state-sync writers (docs/network.md "State sync"): run the exe's own writers on random objects, decode with `opensow netsync`
OBJ2 = 0x62000000
mu.mem_map(OBJ2, 0x10000)
EXTRA = list(mu.mem_read(0x412A48, 31))  # the exe's byte table: per type - 1, which extra follows (0 handle, 1 delay, 2 speed, 3 variant, 4 variant 2, 5 none)
def put(base_, off, v):
    mu.mem_write(base_ + off, struct.pack("<I", v & 0xFFFFFFFF))
def message_bits():
    n = u32(OBJ + 0x4E2C)
    return bytes(mu.mem_read(OBJ + 0xC, (n + 7) // 8)), n
def run_writer(func, name):
    mu.mem_write(OBJ, b"\0" * 0x5000)
    call(func, [OBJ2, OBJ])
    data, n = message_bits()
    return subprocess.check_output([opensow, "netsync", name, data.hex(), str(n)], text=True).split("\n")
for case in range(150):
    mu.mem_write(OBJ2, b"\0" * 0x10000)
    r32 = lambda: rnd.randrange(1 << 32)
    # building: kind 7 bits, word 32, x y 8 bits, owner 2 bits (values wider than the field are cut by the writer)
    vals = [r32() for _ in range(5)]
    for off, v in zip((0x94, 0x90, 4, 8, 0x8C), vals):
        put(OBJ2, off, v)
    got = run_writer(0x40A2D0, "building")
    want = "%d %d %d %d %d" % (vals[0] & 127, vals[1], vals[2] & 255, vals[3] & 255, vals[4] & 3)
    if got[0] != want or got[1] != "bits left 0":
        bad += 1; print("MISMATCH building", got, want)
    # bullet
    ty = rnd.randrange(1, 32)
    v = {o: r32() for o in (0x266C, 0x269C, 0x26A0, 0x26A4, 0x26A8, 0x2698, 0x2660, 0x26B0, 0x26B4, 0x2680, 0x2688, 0x268C, 0x2664, 0x26AC)}
    mu.mem_write(OBJ2, b"\0" * 0x10000)
    put(OBJ2, 0x2658, ty)
    for o, x in v.items():
        put(OBJ2, o, x)
    flag = rnd.randrange(2)
    mu.mem_write(OBJ2 + 0x26C0, bytes([flag]))
    got = run_writer(0x4128F0, "bullet")
    xi = EXTRA[ty - 1]
    xo = {0: (0x2688, 32), 1: (0x26AC, 5), 2: (0x2664, 8), 3: (0x2680, 7), 4: (0x268C, 2)}
    xv = (v[xo[xi][0]] & ((1 << xo[xi][1]) - 1)) if xi in xo else 0
    want = "%d %d %d %d %d %d %d %d %d %d %d %d %d" % (ty, v[0x266C] & 3, v[0x269C] & 4095, v[0x26A0] & 4095, v[0x26A4] & 4095, v[0x26A8] & 4095, v[0x2698] & 65535, v[0x2660] & 65535, v[0x26B0], v[0x26B4] & 63, xi, xv, flag if 19 <= ty <= 21 else -1)
    if got[0] != want or got[1] != "bits left 0":
        bad += 1; print("MISMATCH bullet", ty, got, want)
    # air squad
    mu.mem_write(OBJ2, b"\0" * 0x10000)
    offs = (0x28, 0x1C, 0xC, 0x10, 0x98, 0x9C, 0x20, 0xA0, 0xA4)
    bitsw = (8, 16, 16, 16, 16, 16, 2, 32, 6)
    vals = [r32() for _ in offs]
    for o, x in zip(offs, vals):
        put(OBJ2, o, x)
    got = run_writer(0x4688D0, "air")
    want = " ".join(str(x & ((1 << b) - 1)) for x, b in zip(vals, bitsw))
    if got[0] != want or got[1] != "bits left 0":
        bad += 1; print("MISMATCH air", got, want)
    # unit with and without chassis / top
    mu.mem_write(OBJ2, b"\0" * 0x10000)
    CH, TOP = OBJ2 + 0x2000, OBJ2 + 0x4000
    kind = rnd.choice([1, 7, 15, 40, 41, 46, 109, 110, 111, 112, 113, 0x6C, 0x72])
    handle, player = r32(), rnd.randrange(4)
    ux, uy = rnd.randrange(4096), rnd.randrange(4096)
    home = (rnd.randrange(128), rnd.randrange(128))
    has_ch, has_top = rnd.randrange(2), rnd.randrange(2)
    put(OBJ2, 0x40, kind); put(OBJ2, 0x3C, handle); put(OBJ2, 0x44, player)
    put(OBJ2, 0x14, ux); put(OBJ2, 0x18, uy); put(OBJ2, 0xC, home[0]); put(OBJ2, 0x10, home[1]); put(OBJ2, 0x68, r32())
    put(OBJ2, 8, CH if has_ch else 0); put(OBJ2, 4, TOP if has_top else 0)
    c = [rnd.randrange(0, 128) for _ in range(6)]  # [+0x64 .. +0x78]
    for i, o in enumerate((0x64, 0x68, 0x6C, 0x70, 0x74, 0x78)):
        put(CH, o, c[i])
    goalx, goaly = rnd.randrange(4096), rnd.randrange(4096)
    put(CH, 0x5C, goalx); put(CH, 0x60, goaly)
    w168, w16c, e0 = r32(), r32(), rnd.randrange(4)
    put(CH, 0x168, w168); put(CH, 0x16C, w16c); put(CH, 0xE0, e0)
    face = r32()
    put(TOP, 0x70, face)
    got = run_writer(0x46C5C0, "unit")
    cell = (c[0], c[1]) if has_ch else (ux >> 5, uy >> 5)
    rel = lambda a, b: 0 if a == b else (2 if a > b else 1)
    parts = [kind & 0xFFFF, handle, player & 3, cell[0] & 127, cell[1] & 127, home[0], home[1]]
    line = " ".join(map(str, parts))
    if has_ch:
        line += " C %d %d %d %d %d %d %d %d %d" % (rel(c[0], c[2]), rel(c[1], c[3]), rel(c[2], c[4]), rel(c[3], c[5]), (goalx >> 5) & 127, (goaly >> 5) & 127, w168, w16c & 63, int(e0 == 2))
    else:
        line += " N"
    if has_top:
        line += " T %d %d" % (face & 31, (u32(OBJ2 + 0x68) if 109 <= kind <= 113 else -1))
    else:
        line += " N"
    if got[0] != line or got[1] != "bits left 0":
        bad += 1; print("MISMATCH unit", got, line)
print("net layout:", "ok" if not bad else f"{bad} mismatches")
sys.exit(1 if bad else 0)
