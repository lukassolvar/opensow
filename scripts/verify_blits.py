#!/usr/bin/env python3
"""Check OpenSOW's blit arithmetic (`effects::Blit::apply565`, `opensow blend`) against the exe's own blit routines.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_blits.py "<install>/State of War.exe" target/release/opensow

A random RLE sprite (the exe's in-memory format: u16 w, h, hot x, hot y; row table of u32 word offsets at +0x10; each row is
`u16 segments, u16 first type` then segments `[skip]` (type 0) / `[n, n pixels]` (type 1), alternating) is drawn into a random
RGB565 buffer by each routine with the start-up shade tables built by `0x45d590`; every destination pixel must equal the Rust
result (and untouched pixels must stay).
"""
import random, struct, subprocess, sys
import pefile
from unicorn import UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path); base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, SPR, DST, RET, MASK = 0x70000000, 0x60000000, 0x62000000, 0x50000000, 0x64000000
mu.mem_map(STACK, 0x100000); mu.mem_map(SPR, 0x10000); mu.mem_map(DST, 0x400000); mu.mem_map(RET, 0x1000); mu.mem_map(MASK, 0x400000)
mu.mem_write(RET, b"\xf4")
W, H = 320, 240
def poke(a, v): mu.mem_write(a, struct.pack("<I", v))
poke(0x517AEC, 2); poke(0x517AF4, W - 1); poke(0x49A340, W); poke(0x49A344, H); poke(0x71CEB8, 0); poke(0x71CEBC, 0)
poke(0x49A33B, 1); poke(0x6B3CD4, MASK); poke(0x6B3CD8, W)   # screen mask of 0x45f4d0 / 0x45b5e0: all zero = everything drawn

def call(func, args, count=3_000_000):
    sp = STACK + 0x80000
    for a in reversed(args):
        sp -= 4; mu.mem_write(sp, struct.pack("<I", a))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.emu_start(func, RET, count=count)

call(0x45D590, [], 200_000_000)   # shade / light tables

def sprite(rnd, w, h):
    rows = []; pix = {}
    for y in range(h):
        segs = []; x = 0; typ = rnd.randrange(2)
        first = typ
        while x < w:
            n = rnd.randrange(1, w - x + 1)
            if typ == 0:
                segs.append([n])
            else:
                px = [rnd.randrange(1, 65536) for _ in range(n)]
                for i, v in enumerate(px): pix[(x + i, y)] = v
                segs.append([n] + px)
            x += n; typ ^= 1
        rows.append((first, segs))
    words = []; offs = []
    table_words = 8 + 2 * h          # 0x10 bytes header + 4 bytes per row, in words
    for first, segs in rows:
        offs.append(table_words + len(words))
        words += [len(segs), first]
        for sg in segs: words += sg
    head = struct.pack("<HHhh", w, h, 0, 0) + b"\0" * 8
    tab = b"".join(struct.pack("<HH", o & 0xFFFF, o >> 16) for o in offs)
    return head + tab + struct.pack("<%dH" % len(words), *words), pix

ROUTINES = {"add": 0x45CDE0, "addhalf": 0x45C220, "average": 0x4611F0, "smoke": 0x45F4D0, "darken": 0x45DF40, "plain": 0x45EDF0, "lens": 0x461740}
rnd = random.Random(3)
bad = 0
for name, fn in list(ROUTINES.items()) + [("shade", 0x460F60)]:
    for case in range(12):
        w, h = rnd.randrange(3, 40), rnd.randrange(2, 20)
        data, pix = sprite(rnd, w, h)
        mu.mem_write(SPR, data)
        dst = [rnd.randrange(65536) for _ in range(W * H)]
        mu.mem_write(DST, struct.pack("<%dH" % (W * H), *dst))
        x, y = rnd.randrange(8, W - w - 8), rnd.randrange(8, H - h - 8)
        flag = rnd.randrange(2) if name != "shade" else 1   # shade: flag 1 = the sprite carries (skipped) pixel words
        # 0x4611f0 takes (x, y, sprite, pitch, dst); 0x460f60 (x, y, sprite, pitch, dst, flag); the others (x, y, flag, sprite, pitch, dst)
        call(fn, [x, y, SPR, W, DST] if name in ("average", "lens") else [x, y, SPR, W, DST, flag] if name == "shade" else [x, y, flag, SPR, W, DST])
        got = struct.unpack("<%dH" % (W * H), mu.mem_read(DST, W * H * 2))
        # lens (0x461740): the covered pixel is replaced by the one 2 columns right and 3 rows down, brightened
        off = (3 * W + 2) if name == "lens" else 0
        pairs = [(dst[(y + j) * W + x + i + off], pix.get((i, j))) for j in range(h) for i in range(w)]
        covered = [p for p in pairs if p[1] is not None or name == "shade"]
        # shade: the stencil is the non-skipped runs
        args = []
        for d, s in pairs:
            if s is not None: args += ["%04x" % d, "%04x" % s]
        want = subprocess.check_output([opensow, "blend", name] + args, text=True).split() if args else []
        k = 0
        for j in range(h):
            for i in range(w):
                idx = (y + j) * W + x + i
                s = pix.get((i, j))
                if s is None:
                    if got[idx] != dst[idx] and name != "lens": bad += 1; print("untouched pixel changed", name)
                else:
                    if got[idx] != int(want[k], 16):
                        bad += 1
                        if bad < 10: print("MISMATCH", name, "dst %04x src %04x exe %04x rust %s" % (dst[idx + off], s, got[idx], want[k]))
                    k += 1
        outside = [i for i in range(W * H) if not (x <= i % W < x + w and y <= i // W < y + h) and got[i] != dst[i]]
        if name == "lens": outside = []
        if outside: bad += 1; print("pixels outside the sprite changed", name, len(outside))
    print(name, "checked")
print("blits:", "ok" if not bad else f"{bad} mismatches")
sys.exit(1 if bad else 0)
