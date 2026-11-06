#!/usr/bin/env python3
"""Runs the cloud constructor 0x4145a0, the scatter 0x4149d0 and the per-frame move 0x4147d0 of State of War.exe in an emulator and
compares them with `opensow cloud` (clouds.rs, docs/clouds.md).

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_clouds.py "<install>/State of War.exe" target/release/opensow

The visual generator 0x437cd0 is hooked to return scripted values modulo its argument. The speed table 0x5524a0 is built by the exe's own
routine 0x436df0, which is run first.
"""
import random, struct, subprocess, sys
import pefile
from unicorn import UC_HOOK_CODE, UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path)
base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, RET, HEAP = 0x70000000, 0x50000000, 0x60000000
mu.mem_map(STACK, 0x100000)
mu.mem_map(RET, 0x1000)
mu.mem_map(HEAP, 0x10000)
mu.mem_write(RET, b"\xf4")
script = []


def rand_hook(uc, addr, size, ud):
    sp = uc.reg_read(UC_X86_REG_ESP)
    ret, n = struct.unpack("<II", uc.mem_read(sp, 8))
    uc.reg_write(UC_X86_REG_EAX, script.pop(0) % n if n else 0)
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, rand_hook, begin=0x437CD0, end=0x437CD0)


def call(addr, ecx, args=()):
    sp = STACK + 0x80000
    for a in reversed(args):
        sp -= 4
        mu.mem_write(sp, struct.pack("<I", a & 0xFFFFFFFF))
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.reg_write(UC_X86_REG_ECX, ecx)
    mu.emu_start(addr, RET, count=200000)


def rust(*a):
    return subprocess.check_output([opensow, "cloud", *map(str, a)], text=True).split()


def obj():
    return struct.unpack("<5I", mu.mem_read(HEAP, 20))[1:]


random.seed(1)
bad = 0
for trial in range(300):
    w, h = random.choice([(32, 32), (64, 64), (96, 80), (128, 128)])
    mu.mem_write(0x71CEC0, struct.pack("<II", w, h))
    r = [random.randrange(65536) for _ in range(6)]
    script[:] = r[:4]
    call(0x4145A0, HEAP)
    x, y, sp, pic = obj()
    exp = rust("roll", w, h, *r[:4])
    got = [str(struct.unpack("<i", struct.pack("<I", x))[0]), str(y), str(sp), str(pic)]
    if got != exp:
        bad += 1
        print("roll", w, h, r[:4], got, exp)
    script[:] = r[4:6]
    call(0x4149D0, HEAP)
    x2, y2, _, _ = obj()
    exp = rust("scatter", w, h, *r)
    got = [str(struct.unpack("<i", struct.pack("<I", x2))[0]), str(y2), str(sp), str(pic)]
    if got != exp:
        bad += 1
        print("scatter", w, h, r, got, exp)
print("constructor/scatter mismatches:", bad)
call(0x436DF0, 0)
bad2 = 0
for trial in range(400):
    w = random.choice([32, 64, 96, 128])
    mu.mem_write(0x71CEC0, struct.pack("<II", w, 64))
    speed = random.randrange(10, 40)
    x0 = random.randrange(-800, (w + 25) * 32 + 60)
    t0, n = random.randrange(60), random.randrange(0, 9)
    mu.mem_write(HEAP, struct.pack("<IiiII", 0, x0, 77, speed, 0))
    mu.mem_write(0x552494, struct.pack("<I", n))
    for i in range(n):
        mu.mem_write(0x517B10 + 4 * i, struct.pack("<I", (t0 + i) % 60))
    call(0x4147D0, HEAP)
    alive = mu.reg_read(UC_X86_REG_EAX) & 0xFF
    nx = struct.unpack("<i", mu.mem_read(HEAP + 4, 4))[0]
    got = [str(nx), str(alive)]
    exp = rust("step", x0, speed, w, t0, n)
    if got != exp:
        bad2 += 1
        print("step", x0, speed, w, t0, n, got, exp)
print("move mismatches:", bad2)
bad += bad2
sys.exit(1 if bad else 0)
