#!/usr/bin/env python3
"""Check OpenSOW's multiplayer packet layouts (`formats::net`) against the exe's own bit writers and readers.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_net.py "<install>/State of War.exe" target/release/opensow

  * 0x451540 / 0x451920 (CNetMessage2 key and mouse writers) build random input packages; `opensow netpkg` must decode them.
  * 0x4500d0 / 0x44fe00 (the readers) must give the same values on those packages.
  * 0x4527c0 (header field writer) must set exactly the bits of `net::HEADER_FIELDS`.
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
print("net layout:", "ok" if not bad else f"{bad} mismatches")
sys.exit(1 if bad else 0)
