#!/usr/bin/env python3
"""Shared emulator setup of the savegame store checks (`verify_save_store.py`: one record class at a time, `verify_save_state.py`:
the whole state stream): the exe image in unicorn, an in-memory CArchive (`archive()`, `run()`), vtable lookup and random object
builders (`build()`, which also records the values the store branch must produce in `exp`). Run as the calling script's module:
`sys.argv[1]` is the exe, `sys.argv[2]` the `opensow` binary."""
import os, random, struct, subprocess, sys, tempfile

import pefile
from unicorn import UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]  # the calling script is run as: script.py <exe> <opensow>
pe = pefile.PE(exe_path)
base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
image = bytearray(pe.OPTIONAL_HEADER.SizeOfImage)
for s in pe.sections:
    d = s.get_data()
    mu.mem_write(base + s.VirtualAddress, d)
    image[s.VirtualAddress:s.VirtualAddress + len(d)] = d
STACK, HEAP, ARCH, BUF, RET = 0x70000000, 0x60000000, 0x62000000, 0x64000000, 0x50000000
mu.mem_map(0, 0x1000)  # fs:[0], the SEH frame chain
mu.mem_map(STACK, 0x100000)
mu.mem_map(HEAP, 0x400000)
mu.mem_map(ARCH, 0x1000)
mu.mem_map(BUF, 0x1000000)
mu.mem_map(RET, 0x1000)
mu.mem_write(RET, b"\xf4")

# class -> (Serialize address, object size, vtable-lookup)
SER = {"Unit": 0x46caf0, "Top": 0x47b5e0, "Chassis": 0x4709a0, "Order": 0x478f80, "Weapon": 0x47c700, "Building": 0x40b160,
       "BuildingPart": 0x40e9f0, "Bullet": 0x412a70, "Aircraft": 0x468c30, "Decal": 0x41a1f0, "Cloud": 0x414820,
       "PlayerState": 0x455700, "Stock": 0x419100, "Params": 0x465220, "MapItem": 0x43e480, "SpeechItem": 0x404890}
SIZE = {"Unit": 0xa0, "Top": 0x218, "Chassis": 0x174, "Order": 0x1c, "Weapon": 0x48, "Building": 0x3a0, "BuildingPart": 0x28,
        "Bullet": 0x26cc, "Aircraft": 0xb4, "Decal": 0x20, "Cloud": 0x20, "PlayerState": 0xc68, "Stock": 0x338, "Params": 0x80,
        "MapItem": 0x30, "SpeechItem": 0x18}


def vtable_for(fn):
    """The vtable (in .rdata) whose slot 2 is `fn`."""
    t = struct.pack("<I", fn)
    i = 0
    while True:
        i = image.find(t, i)
        if i < 0:
            return 0  # called directly, not through a vtable (the global players / stock / params blocks)
        if i >= 8 and i % 4 == 0:
            vt = base + i - 8
            # slot 0 and 1 must be code addresses inside the image
            a, b = struct.unpack("<II", image[i - 8:i])
            if base <= a < base + len(image) and base <= b < base + len(image):
                return vt
        i += 4


VT = {c: vtable_for(f) for c, f in SER.items()}
rnd = random.Random(20261106)
heap_top = HEAP


def reset_heap():
    global heap_top
    heap_top = HEAP


def alloc(n):
    global heap_top
    a = heap_top
    heap_top += (n + 15) & ~15
    return a


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", mu.mem_read(a, 4))[0]


# --- scalar tables (the Rust ones, parsed from `opensow savelayout`) ---
layout = {}
for line in subprocess.check_output([opensow, "savelayout"], text=True).splitlines():
    f = line.split()
    layout[f[0]] = [(x[0], int(x[1:], 16)) for x in f[3:]]

exp = {}  # (path, offset) -> value


def put_scalars(cls, addr, path, skip=()):
    for k, off in layout[cls]:
        if off in skip:
            continue
        if k == "b":
            v = rnd.choice([0, 1, 2, 3, 0x80])
            mu.mem_write(addr + off, bytes([v]))
            exp[(path, off)] = 1 if v == 1 else 0
        else:
            v = rnd.getrandbits(32)
            w32(addr + off, v)
            exp[(path, off)] = v


def bools(addr, path, offs):
    for off in offs:
        v = rnd.choice([0, 1, 2, 3, 0x80])
        mu.mem_write(addr + off, bytes([v]))
        exp[(path, off)] = 1 if v == 1 else 0


def words(addr, path, offs):
    for off in offs:
        v = rnd.getrandbits(32)
        w32(addr + off, v)
        exp[(path, off)] = v


def build(cls, addr, path=""):
    """Fills the object at `addr` (already zeroed, size SIZE[cls]) and records the values the store branch must produce."""
    w32(addr, VT[cls])
    put_scalars(cls, addr, path, skip=(0xc,) if cls == "Chassis" else ())
    if cls == "Chassis":
        kind = rnd.choice([0x28, 0x28, 5, 0x1a])
        w32(addr + 0xc, kind)
        exp[(path, 0xc)] = kind
    if cls == "Unit":
        bools(addr, path, [0x48, 0x49, 0x4a])
        for off, sub in ((4, "Top"), (8, "Chassis")):
            if rnd.random() < 0.8:
                p = alloc(SIZE[sub])
                mu.mem_write(p, b"\0" * SIZE[sub])
                w32(addr + off, p)
                build(sub, p, f"{path}/child{off:x}")
            else:
                w32(addr + off, 0)
    elif cls == "Top":
        words(addr, path, [0x94 + 4 * i for i in range(10)])
        words(addr, path, [0xcc + 0x10 * i + 4 * k for i in range(20) for k in range(4)])
        build("Weapon", addr + 8, f"{path}/child8")
    elif cls == "Chassis":
        build("Order", addr + 0xd8, f"{path}/childd8")
        n = rnd.randrange(0, 5)
        arr = alloc(0x1c * max(n, 1))
        mu.mem_write(arr, b"\0" * (0x1c * max(n, 1)))
        w32(addr + 0xc8, arr)
        w32(addr + 0xcc, n)
        exp[(path, 0xcc)] = n
        for i in range(n):
            build("Order", arr + 0x1c * i, f"{path}/list{i}")
        for ptr_off, cnt_off, name in ((0x88, 0x8c, "route"), (0x9c, 0xa0, "local")):
            n = rnd.randrange(0, 6)
            arr = alloc(8 * max(n, 1))
            w32(addr + ptr_off, arr)
            w32(addr + cnt_off, n)
            exp[(path, cnt_off)] = n
            for i in range(2 * n):
                v = rnd.getrandbits(32)
                w32(arr + 4 * i, v)
                exp[(f"{path}/{name}", i)] = v
        if r32(addr + 0xc) == 0x28:
            build("Weapon", addr + 0x104, f"{path}/child104")
    elif cls == "Building":
        bools(addr, path, [0x88, 0x89, 0x8a])
        words(addr, path, [0x7c + 4 * k for k in range(3)] + [0x98 + 4 * i for i in range(25)])
        for k in range(5):
            build("BuildingPart", addr + 0x15c + 0x28 * k, f"{path}/child{0x15c + 0x28 * k:x}")
        words(addr, path, [0x224 + 4 * i for i in range(8)] + [0x244 + 4 * i for i in range(8)])
        words(addr, path, [0x26c + 4 * i for i in range(10)] + [0x294 + 4 * i for i in range(10)])
        n = rnd.randrange(0, 11)
        w32(addr + 0x394, n)
        exp[(path, 0x394)] = n
        words(addr, path, [0x2cc + 0x14 * j + 4 * k for j in range(n) for k in range(5)])
    elif cls == "Bullet":
        words(addr, path, [0xc + 4 * i for i in range(15)] + [0x48 + 4 * i for i in range(15)] + [0x84 + 4 * i for i in range(15)])
        n = 2 * rnd.randrange(1, 20)
        prog = rnd.randrange(0, n + 1)
        w32(addr + 0x2648, n)
        w32(addr + 0x2644, prog)
        exp[(path, 0x2648)] = n
        exp[(path, 0x2644)] = prog
        words(addr, path, [0xc4 + 4 * i for i in range(n // 2)])
        words(addr, path, [0xd44 + 4 * i for i in range(prog, n)])
        words(addr, path, [0x2698, 0x269c, 0x26a0, 0x26a4, 0x26a8, 0x26ac, 0x26b0, 0x26b4, 0x26bc])
        bools(addr, path, [0x26b8, 0x26c0])
    elif cls == "Aircraft":
        bools(addr, path, [0x8c + k for k in range(5)])
    elif cls == "PlayerState":
        words(addr, path, [0x10 + 4 * i for i in range(10)])
    elif cls == "Stock":
        words(addr, path, [0x2e4 + 8 * i for i in range(9)] + [0x2e8 + 8 * i for i in range(9)])
        for i in range(10):
            n = rnd.randrange(0, 5)
            arr = alloc(4 * max(n, 1))
            w32(addr + 0x218 + 0x14 * i, arr)
            w32(addr + 0x21c + 0x14 * i, n)
            exp[(path, 0x21c + 0x14 * i)] = n
            for j in range(n):
                v = rnd.getrandbits(32)
                w32(arr + 4 * j, v)
                exp[(path, (i + 1) << 16 | j)] = v
    elif cls == "Params":
        words(addr, path, [4 + 4 * i for i in range(30)] + [0x7c])
    elif cls == "MapItem":
        pass  # handled by the kind loop below


def archive():
    mu.mem_write(ARCH, b"\0" * 0x100)
    w32(ARCH + 0x18, 0)  # storing
    w32(ARCH + 0x28, BUF)
    w32(ARCH + 0x2C, BUF + 0x1000000)


def run(cls, addr):
    archive()
    sp = STACK + 0x100000 - 0x1000
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", ARCH))
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.reg_write(UC_X86_REG_ECX, addr)
    mu.emu_start(SER[cls], RET, count=5_000_000)
    end = r32(ARCH + 0x28)
    return bytes(mu.mem_read(BUF, end - BUF))




# ---- MFC CString / CArchive helper emulations shared by verify_save_state.py and verify_save_load.py ----
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP

STR = 0x65000000
mu.mem_map(STR, 0x100000)
str_top = STR
STUBS = 0x66000000
mu.mem_map(STUBS, 0x1000)


def new_cstring_data(text):
    """Chars with the CStringData header (manager, length, alloc length, refs) before them; returns the char pointer."""
    global str_top
    b = text.encode()
    a = str_top
    str_top += 16 + len(b) + 1 + 16
    str_top = (str_top + 15) & ~15
    mu.mem_write(a, struct.pack("<IiiI", 0, len(b), len(b), 1) + b + b"\0")
    return a + 16


def set_cstring(obj, text):
    w32(obj, new_cstring_data(text))


def ret_to(uc, pop):
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, ret)


def arg(uc, i):
    sp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack("<I", uc.mem_read(sp + 4 * (i + 1), 4))[0]


def put_bytes(ar, data):
    cur = r32(ar + 0x28)
    mu.mem_write(cur, data)
    w32(ar + 0x28, cur + len(data))


def stub(addr, fn):
    mu.hook_add(UC_HOOK_CODE, lambda uc, a, s, ud: fn(uc), begin=addr, end=addr)


# IAT slots of the CString / CArchive helpers used by the store path, redirected to stubs placed in STUBS
def iat(slot, index, fn):
    target = STUBS + 0x10 * index
    mu.mem_write(target, b"\xc3")
    w32(slot, target)
    stub(target, fn)


def cs_default_ctor(uc):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    set_cstring(ecx, "")
    uc.reg_write(UC_X86_REG_EAX, ecx)
    ret_to(uc, 0)


def cs_ctor_lit(uc):  # CString(const char*)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    p = arg(uc, 0)
    s = bytes(mu.mem_read(p, 64)).split(b"\0")[0].decode("latin1")
    set_cstring(ecx, s)
    uc.reg_write(UC_X86_REG_EAX, ecx)
    ret_to(uc, 4)


def cs_get_length(uc):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    uc.reg_write(UC_X86_REG_EAX, r32(r32(ecx) - 12))
    ret_to(uc, 0)


def cs_get_string(uc):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    uc.reg_write(UC_X86_REG_EAX, r32(ecx))
    ret_to(uc, 0)


def cs_dtor(uc):
    ret_to(uc, 0)


iat(0x485758, 0, cs_default_ctor)
iat(0x485740, 1, cs_ctor_lit)
iat(0x4857d4, 2, cs_get_length)
iat(0x4857d8, 3, cs_get_string)
iat(0x4857bc, 4, cs_dtor)


def write_string_length(uc):  # AfxWriteStringLength(ar, length, unicode)
    ar, n = arg(uc, 0), arg(uc, 1)
    if n < 0xFF:
        put_bytes(ar, bytes([n]))
    elif n < 0xFFFE:
        put_bytes(ar, b"\xff" + struct.pack("<H", n))
    else:
        put_bytes(ar, b"\xff\xff\xff" + struct.pack("<I", n))
    ret_to(uc, 12)


def ar_write(uc):  # CArchive::Write(buf, n)
    ar = uc.reg_read(UC_X86_REG_ECX)
    buf, n = arg(uc, 0), arg(uc, 1)
    put_bytes(ar, bytes(mu.mem_read(buf, n)))
    ret_to(uc, 8)


# 0x456770 reloads the campaign object from the progress file before the state is written (CFile access); the object's current
# content is what 0x456ac0 serialises, so the reload is skipped here
stub(0x456770, lambda uc: ret_to(uc, 0))
stub(0x47D37B, write_string_length)
stub(0x47D351, ar_write)

