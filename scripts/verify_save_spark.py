#!/usr/bin/env python3
"""Runs 0x443210 (next vertex of a Load/Save page spark) of State of War.exe in an x86 emulator and compares it with `opensow spark`.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_save_spark.py "<install>/State of War.exe" target/release/opensow

Coordinates are relative to the screen centre (the table columns/rows are written into the exe's globals `0x71c4e8..f4` / `0x71c510` that way);
the visual generator 0x437cd0 is hooked to return a scripted value modulo its argument. Outputs the exe does not write stay at a sentinel ("none").
"""
import random, struct, subprocess, sys
import pefile
from unicorn import UC_HOOK_CODE, UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path)
base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, RET = 0x70000000, 0x50000000
mu.mem_map(STACK, 0x100000)
mu.mem_map(RET, 0x1000)
mu.mem_write(RET, b"\xf4")
COLS, ROW0 = [-300, -270, -45, 104], -160
for i, c in enumerate(COLS):
    mu.mem_write(0x71C4E8 + 4 * i, struct.pack("<i", c))
mu.mem_write(0x71C510, struct.pack("<i", ROW0))
script = [0]


def rand_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    ret, n = struct.unpack("<II", uc.mem_read(sp, 8))
    uc.reg_write(UC_X86_REG_EAX, script[0] % n if n else 0)
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, rand_hook, begin=0x437CD0, end=0x437CD0)
SENT = 0x7FFFFFFF


def exe_next(px, py, x, y, r):
    script[0] = r
    sp = STACK + 0x80000
    out = STACK + 0x90000
    mu.mem_write(out, struct.pack("<ii", SENT, SENT))
    args = [px, py, x, y, out, out + 4]
    for a in reversed(args):
        sp -= 4
        mu.mem_write(sp, struct.pack("<i", a & 0xFFFFFFFF if a < 0 else a) if a >= 0 else struct.pack("<i", a))
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.emu_start(0x443210, RET, count=100000)
    ox, oy = struct.unpack("<ii", mu.mem_read(out, 8))
    return "none" if (ox, oy) == (SENT, SENT) else f"{ox} {oy}"


def vertex():
    return random.choice(COLS), ROW0 + 32 * random.randint(0, 10)


bad = total = 0
for _ in range(1500):
    cur = vertex()
    k = random.random()
    if k < 0.25:
        prev = cur
    elif k < 0.9:  # a neighbour
        dx, dy = random.choice([(1, 0), (-1, 0), (0, 1), (0, -1)])
        ci = COLS.index(cur[0]) + dx
        ri = (cur[1] - ROW0) // 32 + dy
        prev = (COLS[ci], ROW0 + 32 * ri) if 0 <= ci < 4 and 0 <= ri <= 10 else cur
    else:
        prev = vertex()
        if random.random() < 0.3:
            cur = (cur[0] + 1, cur[1])
    r = random.randint(0, 20)
    e = exe_next(*prev, *cur, r)
    m = subprocess.run([opensow, "spark", str(prev[0]), str(prev[1]), str(cur[0]), str(cur[1]), str(r)], capture_output=True, text=True).stdout.strip()
    total += 1
    if e != m:
        bad += 1
        if bad < 6:
            print("MISMATCH", prev, cur, r, "exe:", e, "rust:", m)
print(f"spark next vertex (0x443210): {total - bad}/{total} match")
sys.exit(1 if bad else 0)
