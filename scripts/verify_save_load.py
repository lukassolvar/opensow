#!/usr/bin/env python3
"""Run the exe's **load** branch of the savegame state function (`0x42a400` with a loading archive) on state streams, then its store
branch on the loaded objects, and require the stream to come back unchanged.

    /tmp/v/bin/python scripts/verify_save_load.py "<install>/State of War.exe" target/release/opensow [stream ...]

The streams are the `M05.state` files that `OPENSOW_DUMP_STATES=dir cargo test -p opensow-game a_mission_round` writes (OpenSOW's
`World::to_state`); the check is that **the exe accepts our streams**: every tag and header is verified by
the exe's own code (a mismatch would reach its "savegame is corrupted" error, which is trapped), the objects are constructed by the
exe's constructors and deserialised by its own `Serialize` methods, and storing them again gives the same bytes (the 317-byte campaign
record is masked: the exe re-randomises its filler).

Replaced by small emulations (they need files, MFC or the level): the level loader `0x4267d0`, operator new/delete, the MFC CString and
CArray helpers, the string reader `0x404440`, the campaign file access `0x456770` / `0x456720` / `0x456e50`, the cloud trim `0x453060`.
"""
import os, struct, subprocess, sys

from savestore_lib import *
import verify_save_state as vs  # only for its `run_state` (the exe's store branch)

LOADBUF, NEWHEAP = 0x68000000, 0x69000000
mu.mem_map(LOADBUF, 0x1000000)
mu.mem_map(NEWHEAP, 0x4000000)
new_top = NEWHEAP
errors = []


def new_alloc(n):
    global new_top
    a = new_top
    new_top += (n + 15) & ~15
    mu.mem_write(a, b"\0" * n)
    return a


def cdecl_ret(uc, value=0):
    uc.reg_write(UC_X86_REG_EAX, value)
    sp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(sp, 4))
    uc.reg_write(UC_X86_REG_ESP, sp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def read_string_into(uc):  # 0x404440: thiscall(archive), arg = CString*; reads an MFC string
    ar = uc.reg_read(UC_X86_REG_ECX)
    target = arg(uc, 0)
    cur = r32(ar + 0x28)
    n = mu.mem_read(cur, 1)[0]
    cur += 1
    if n == 0xFF:
        n = struct.unpack("<H", mu.mem_read(cur, 2))[0]
        cur += 2
        if n == 0xFFFF:
            n = r32(cur)
            cur += 4
    text = bytes(mu.mem_read(cur, n)).decode("latin1")
    w32(ar + 0x28, cur + n)
    set_cstring(target, text)
    ret_to(uc, 4)


def cs_compare(uc):  # CString::Compare(const char*): 0 when equal
    ecx = uc.reg_read(UC_X86_REG_ECX)
    p = arg(uc, 0)
    want = bytes(mu.mem_read(p, 64)).split(b"\0")[0]
    have_ptr = r32(ecx)
    have = bytes(mu.mem_read(have_ptr, r32(have_ptr - 12)))
    if have != want:
        errors.append(f"tag {have!r} != {want!r}")
    uc.reg_write(UC_X86_REG_EAX, 0 if have == want else 1)
    ret_to(uc, 4)


def op_new(uc):
    n = arg(uc, 0)
    cdecl_ret(uc, new_alloc(n))


def op_delete(uc):
    cdecl_ret(uc, 0)


def array_set_size(uc):  # CArray::SetSize(n, grow): data +4, size +8
    ecx = uc.reg_read(UC_X86_REG_ECX)
    n = arg(uc, 0)
    w32(ecx + 8, n)
    if n and not r32(ecx + 4):
        w32(ecx + 4, new_alloc(max(4 * n, 64)))
    ret_to(uc, 8)


def array_add(uc):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    v = arg(uc, 0)
    if not r32(ecx + 4):
        w32(ecx + 4, new_alloc(4 * 4096))
    n = r32(ecx + 8)
    w32(r32(ecx + 4) + 4 * n, v)
    w32(ecx + 8, n + 1)
    uc.reg_write(UC_X86_REG_EAX, n)
    ret_to(uc, 4)


def level_loader(uc):  # 0x4267d0(level, CString, int): cdecl; it records the level number and succeeds
    w32(0x49A33C, arg(uc, 0))
    cdecl_ret(uc, 1)


def error_box(uc):  # 0x41c2b0: the exe's message box before it throws
    errors.append("the exe reported an error (0x41c2b0)")
    cdecl_ret(uc, 0)


def read_string_length(uc):  # AfxReadStringLength(ar, &unicode): reads the MFC length prefix, ANSI
    ar, flag = arg(uc, 0), arg(uc, 1)
    cur = r32(ar + 0x28)
    n = mu.mem_read(cur, 1)[0]
    cur += 1
    if n == 0xFF:
        n = struct.unpack("<H", mu.mem_read(cur, 2))[0]
        cur += 2
        if n == 0xFFFF:
            n = r32(cur)
            cur += 4
    w32(ar + 0x28, cur)
    w32(flag, 1)  # character size in bytes: the stream holds ANSI strings
    uc.reg_write(UC_X86_REG_EAX, n)
    ret_to(uc, 8)


def archive_read(uc):  # CArchive::Read(buf, n): thiscall
    ar = uc.reg_read(UC_X86_REG_ECX)
    buf, n = arg(uc, 0), arg(uc, 1)
    cur = r32(ar + 0x28)
    mu.mem_write(buf, bytes(mu.mem_read(cur, n)))
    w32(ar + 0x28, cur + n)
    uc.reg_write(UC_X86_REG_EAX, n)
    ret_to(uc, 8)


def cs_ctor_buf(uc):  # CString(const char* buf, int n)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    buf, n = arg(uc, 0), arg(uc, 1)
    set_cstring(ecx, bytes(mu.mem_read(buf, n)).decode("latin1"))
    uc.reg_write(UC_X86_REG_EAX, ecx)
    ret_to(uc, 8)


def cs_assign(uc):  # CString::operator=(const CString&)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    src = arg(uc, 0)
    n = r32(r32(src) - 12)
    set_cstring(ecx, bytes(mu.mem_read(r32(src), n)).decode("latin1"))
    uc.reg_write(UC_X86_REG_EAX, ecx)
    ret_to(uc, 4)


iat(0x485754, 7, cs_ctor_buf)
iat(0x4857B8, 8, cs_assign)
def memcpy_(uc):  # the CRT's memcpy picks an implementation from CPU feature words that the emulation never initialised
    d, sr, n = arg(uc, 0), arg(uc, 1), arg(uc, 2)
    mu.mem_write(d, bytes(mu.mem_read(sr, n)))
    cdecl_ret(uc, d)


def memset_(uc):
    d, v, n = arg(uc, 0), arg(uc, 1), arg(uc, 2)
    mu.mem_write(d, bytes([v & 0xFF]) * n)
    cdecl_ret(uc, d)


iat(0x4857F8, 9, op_new)  # mfc140 operator new
stub(0x47EDA8, memcpy_)
stub(0x47EDB4, memset_)
stub(0x47D34B, archive_read)
stub(0x47D375, read_string_length)
stub(0x404440, read_string_into)
iat(0x4853E4, 5, cs_compare)
iat(0x4857C0, 6, cs_ctor_lit)
stub(0x47D315, op_new)
stub(0x47D321, op_delete)
stub(0x47D30F, op_delete)  # operator delete (the old cloud list is destroyed before a load)
stub(0x47D387, array_set_size)
stub(0x47D38D, array_add)
stub(0x4267D0, level_loader)
stub(0x41C2B0, error_box)
stub(0x453060, lambda uc: ret_to(uc, 0))
stub(0x456720, lambda uc: ret_to(uc, 4))
stub(0x456E50, lambda uc: (uc.reg_write(UC_X86_REG_EAX, 0), ret_to(uc, 0)))


def load(stream, speech_objects):
    reset_heap()
    archive()
    w32(ARCH + 0x18, 1)  # loading
    mu.mem_write(LOADBUF, stream)
    w32(ARCH + 0x28, LOADBUF)
    w32(ARCH + 0x2C, LOADBUF + len(stream))
    w32(0x575720, speech_objects - 1)
    w32(0x575730, 0)
    set_cstring(0x71CEC8, "VER7")
    errors.clear()
    sp = STACK + 0x100000 - 0x1000
    sp -= 4; mu.mem_write(sp, struct.pack("<I", ARCH))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    mu.emu_start(0x42A400, RET, count=400_000_000)
    return r32(ARCH + 0x28) - LOADBUF


def store():
    return vs.run_state()


def mask(stream):
    """The campaign record (317 bytes) in front of the first tag, whose filler the exe re-randomises."""
    t = stream.find(b"\x04P2v3")
    return stream[: t - 317] + stream[t:]


def fresh_image():
    """Every stream starts from the pristine image: the real level loader (stubbed here) would reset the map and the grids, so leftovers
    of the previous stream would otherwise show up as differences."""
    for sec in pe.sections:
        if sec.Characteristics & 0x80000000:  # writable sections only: the import table (with our stubs) stays
            a = sec.VirtualAddress
            n = (max(sec.Misc_VirtualSize, sec.SizeOfRawData) + 0xFFF) & ~0xFFF
            mu.mem_write(base + a, bytes(image[a:a + n]))
    mu.mem_write(NEWHEAP, b"\0" * 0x400000)


def check(name, stream, speech_objects=1):
    global new_top
    fresh_image()
    new_top = NEWHEAP
    mu.mem_write(0x575720, b"\0" * 4)
    try:
        used = load(stream, speech_objects)
    except Exception as ex:
        sp = mu.reg_read(UC_X86_REG_ESP)
        print(f"{name}: load stopped: {ex} at {mu.reg_read(UC_X86_REG_EIP):#x}, stack {[hex(r32(sp + 4 * i)) for i in range(6)]}")
        return False
    if errors:
        print(f"{name}: {errors[:3]}")
        return False
    if used != len(stream):
        print(f"{name}: the load read {used} of {len(stream)} bytes")
        return False
    again = store()
    if mask(again) != mask(stream):
        d = next(i for i in range(min(len(again), len(stream))) if mask(again)[i] != mask(stream)[i]) if mask(again)[: min(len(again), len(stream))] != mask(stream)[: min(len(again), len(stream))] else min(len(again), len(stream))
        print(f"{name}: stored {len(again)} bytes vs {len(stream)}, first difference at {d}: {mask(stream)[d:d+12].hex()} vs {mask(again)[d:d+12].hex()}")
        return False
    print(f"{name}: loaded {used} bytes, stored them back identically")
    return True


def main():
    files = sys.argv[3:]
    if not files:
        sys.exit("give state streams: OPENSOW_DUMP_STATES=dir cargo test -p opensow-game a_mission_round, then pass dir/*.state "
                 "(streams of the exe's own store branch from random states are no use here: their random positions break the post-load grid code)")
    ok = True
    for f in files:
        ok &= check(os.path.basename(f), open(f, "rb").read())
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
