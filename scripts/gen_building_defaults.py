#!/usr/bin/env python3
"""Runs the exe's building constructor `0x40a330` (stdcall(x, y, kind, owner, slot, handle) on a fresh object) for every turret and
building kind and prints the serialised fields it sets as a Rust table (`BUILDING_DEFAULTS` in
`crates/opensow-game/src/sim/savestate.rs`): the values a freshly built object holds, which `World::to_state` writes for the
fields the sim has no counterpart for.

    /tmp/v/bin/python scripts/gen_building_defaults.py "<install>/State of War.exe" target/release/opensow
"""
import struct, subprocess, sys
from savestore_lib import *
from unicorn.x86_const import UC_X86_REG_EIP

# the constructor registers the building as a static map sprite (0x458ce0(5 args)): not part of the object
stub(0x458CE0, lambda uc: ret_to(uc, 0x14))
def cdecl_ret(uc, value):
    uc.reg_write(UC_X86_REG_EAX, value)
    ret_to(uc, 0)


def memcpy_(uc):  # the CRT picks its memcpy from CPU feature words the emulation never sets
    d, sr, n = arg(uc, 0), arg(uc, 1), arg(uc, 2)
    mu.mem_write(d, bytes(mu.mem_read(sr, n)))
    cdecl_ret(uc, d)


def memset_(uc):
    d, v, n = arg(uc, 0), arg(uc, 1), arg(uc, 2)
    mu.mem_write(d, bytes([v & 0xFF]) * n)
    cdecl_ret(uc, d)


stub(0x47EDA8, memcpy_)
stub(0x47EDB4, memset_)
OBJ = alloc(0x400)
KINDS = [25, 26, 27, 28, 29, 100, 101, 102, 103, 104, 105, 106, 107, 108]
scalar_offsets = {off for k, off in layout["Building"]}
# loops of the Building record that the constructor may fill too
for i in range(25):
    scalar_offsets.add(0x98 + 4 * i)
for i in range(8):
    scalar_offsets |= {0x224 + 4 * i, 0x244 + 4 * i}
for i in range(10):
    scalar_offsets |= {0x26c + 4 * i, 0x294 + 4 * i}
for k in range(3):
    scalar_offsets |= {0x88 + k, 0x7c + 4 * k}
SKIP = {0x4, 0x8, 0x8c, 0x90, 0x94}  # cell, owner, handle, kind: the bridge sets them from the structure
rows = []
for kind in KINDS:
    reset_heap()
    mu.mem_write(OBJ, b"\0" * 0x400)
    sp = STACK + 0x100000 - 0x1000
    for a in reversed([5, 6, kind, 1, 0, 0x1234]):  # x, y, kind, owner, slot, handle (non-zero: no allocation)
        sp -= 4
        mu.mem_write(sp, struct.pack("<I", a))
    sp -= 4
    mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.reg_write(UC_X86_REG_ECX, OBJ)
    try:
        mu.emu_start(0x40A330, RET, count=2_000_000)
    except Exception as ex:
        sp = mu.reg_read(UC_X86_REG_ESP)
        sys.exit(f"kind {kind}: {ex} at {mu.reg_read(UC_X86_REG_EIP):#x}, stack {[hex(r32(sp + 4 * i)) for i in range(5)]}")
    mem = bytes(mu.mem_read(OBJ, 0x400))
    out = []
    for off in sorted(scalar_offsets - SKIP):
        # bools are single bytes in the record; the neighbouring bytes belong to other fields, so read the width the record uses
        is_bool = any(k == "b" and o == off for k, o in layout["Building"]) or (0x88 <= off <= 0x8a)
        v = mem[off] if is_bool else struct.unpack("<I", mem[off:off + 4])[0]
        if v:
            out.append((off, v))
    rows.append((kind, out))
print("/// Fields the exe's building constructor `0x40a330` sets, per kind (scripts/gen_building_defaults.py): `(offset, value)`, zeros left out.")
print("pub const BUILDING_DEFAULTS: &[(i32, &[(u32, u32)])] = &[")
for kind, out in rows:
    print(f"    ({kind}, &[" + ", ".join(f"({o:#x}, {v:#x})" for o, v in out) + "]),")
print("];")
