#!/usr/bin/env python3
"""What the input record's ten `CArray`s are for (docs/savegame.md "Global serializers"): the three input records (`0x338` bytes per
player, saved with magic `0x7b33`) each hold ten MFC `CDWordArray`s at `+0x214 + 0x14k` (vtable, data `+4`, size `+8`). This script
finds every instruction of the exe that can reach them and runs the ones that do in an emulator:

    venv/bin/python scripts/verify_save_input_arrays.py "<install>/State of War.exe" ["<Warmonger>/State of War Warmonger.exe"]

1. Static, over the whole code section: no absolute address points into any record's array block, and every displacement or
   immediate in `[0x214, 0x2dc)` (the block's offsets) inside the record class's code lies in one of the functions of `EXPECTED`.
2. Emulated: the constructor builds ten arrays and empties each (`SetSize(0, -1)`), the new-mission reset empties them all, the
   destructors destroy them, and `Serialize` writes / reads a count and the items for each; a freshly reset record stores ten zero
   counts. Nothing else in the exe adds an item, so every save of the original holds ten empty arrays.
MFC (`mfc140.dll`, by ordinal) is replaced by small emulations of the calls these functions make.
"""
import struct, sys

import capstone
import pefile
from capstone import x86
from unicorn import UC_ARCH_X86, UC_HOOK_CODE, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

STRIDE, LO, HI, N = 0x338, 0x214, 0x2DC, 10
# function -> role, per exe (keyed by the record table's address). The one non-array hit is a building's rally point: the base
# register there holds the result of the handle lookup (base `0x43a910`, Warmonger `0x439270`), not an input record.
EXPECTED = {
    0x6BED08: {0x414A70: "ctor", 0x415020: "create", 0x414CF0: "dtor", 0x414D50: "dtor", 0x414E20: "reset", 0x419100: "serialize",
               0x416880: "rally"},
    0x6B5AE8: {0x414470: "ctor", 0x414A20: "create", 0x4146F0: "dtor", 0x414750: "dtor", 0x414820: "reset", 0x418A50: "serialize",
               0x4161F0: "rally"},
}
ORD_STR_CTOR, ORD_STR_DTOR, ORD_ARR_CTOR, ORD_ARR_DTOR, ORD_SETSIZE, ORD_SETATGROW, ORD_NEW, ORD_DELETE = 316, 1044, 484, 1129, 13880, 13195, 1507, 1509


class Exe:
    def __init__(self, path):
        self.pe = pe = pefile.PE(path)
        self.base = pe.OPTIONAL_HEADER.ImageBase
        self.img = pe.get_memory_mapped_image()
        text = next(s for s in pe.sections if s.Characteristics & 0x20000000)
        md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        md.skipdata, md.detail = True, True
        self.code = [i for i in md.disasm(self.img[text.VirtualAddress:text.VirtualAddress + text.Misc_VirtualSize],
                                          self.base + text.VirtualAddress) if i.id]
        lo, hi = self.base + text.VirtualAddress, self.base + text.VirtualAddress + text.Misc_VirtualSize
        starts = {i.operands[0].imm for i in self.code if i.mnemonic == "call" and i.operands[0].type == x86.X86_OP_IMM}
        # constructors, destructors and virtual methods are only referenced as values (pushed, or in a vtable): any code address
        # after int3 padding that the image holds as an immediate or a data word
        held = {op.imm & 0xFFFFFFFF for i in self.code for op in i.operands if op.type == x86.X86_OP_IMM}
        for sec in pe.sections:
            if not sec.Characteristics & 0x20000000:
                d = sec.get_data()
                held.update(struct.unpack_from("<I", d, k)[0] for k in range(0, len(d) - 3, 4))
        starts |= {a for a in held if lo <= a < hi and self.img[a - self.base - 1] == 0xCC}
        self.starts = sorted(starts)
        # `push dtor; push ctor; push 3; push 0x338; push table; call` registers the three records
        pat = b"\x6a\x03\x68" + struct.pack("<I", STRIDE) + b"\x68"
        k = self.img.find(pat)
        self.dtor, self.ctor = struct.unpack("<II", self.img[k - 9:k - 5] + self.img[k - 4:k])
        self.table = struct.unpack("<I", self.img[k + 8:k + 12])[0]
        magic = self.img.find(b"\x68\x33\x7b\x00\x00")
        self.serialize = self.function_of(self.base + magic)
        self.ordinal = {i.address: i.ordinal for e in pe.DIRECTORY_ENTRY_IMPORT for i in e.imports}

    def function_of(self, a):
        """The nearest call target at or before `a` (direct calls only reach function entries)."""
        lo, hi = 0, len(self.starts)
        while hi - lo > 1:
            m = (lo + hi) // 2
            lo, hi = (m, hi) if self.starts[m] <= a else (lo, m)
        return self.starts[lo]


def static_hits(e):
    """(1) absolute references into any record's array block; (2) block offsets used inside the class's code."""
    blocks = [(e.table + p * STRIDE + LO, e.table + p * STRIDE + HI) for p in range(3)]
    absolute, offsets = [], []
    lo_code, hi_code = e.ctor, e.serialize + 0x1000  # the class's functions sit together, ctor first, Serialize near the end
    for i in e.code:
        for op in i.operands:
            if op.type == x86.X86_OP_MEM:
                v = op.mem.disp & 0xFFFFFFFF
                if any(a <= v < b for a, b in blocks):
                    absolute.append(i)
                if op.mem.base and LO <= op.mem.disp < HI and lo_code <= i.address < hi_code:
                    offsets.append(i)
            elif op.type == x86.X86_OP_IMM and i.mnemonic in ("add", "sub") and LO <= op.imm < HI and lo_code <= i.address < hi_code:
                offsets.append(i)
    return absolute, offsets


class Emu:
    STACK, HEAP, STUBS, RET = 0x70000000, 0x60000000, 0x66000000, 0x50000000

    def __init__(self, e):
        self.e, self.calls = e, []
        mu = self.mu = Uc(UC_ARCH_X86, UC_MODE_32)
        mu.mem_map(e.base, (e.pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
        mu.mem_write(e.base, bytes(e.img))
        for a, n in ((0, 0x1000), (self.STACK, 0x100000), (self.HEAP, 0x400000), (self.STUBS, 0x10000), (self.RET, 0x1000)):
            mu.mem_map(a, n)
        mu.mem_write(self.RET, b"\xf4")
        self.top = self.HEAP + 0x100000
        # every import slot -> a stub of its own; the hook dispatches by MFC ordinal
        for k, (slot, ordinal) in enumerate(e.ordinal.items()):
            stub = self.STUBS + 4 * k
            mu.mem_write(stub, b"\xc3")
            self.w32(slot, stub)
            mu.hook_add(UC_HOOK_CODE, lambda uc, a, s, o: self.mfc(o), ordinal, stub, stub)

    def w32(self, a, v):
        self.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))

    def r32(self, a):
        return struct.unpack("<I", self.mu.mem_read(a, 4))[0]

    def arg(self, i):
        return self.r32(self.mu.reg_read(UC_X86_REG_ESP) + 4 * (i + 1))

    def ret(self, pop, value=0):
        sp = self.mu.reg_read(UC_X86_REG_ESP)
        self.mu.reg_write(UC_X86_REG_EAX, value)
        self.mu.reg_write(UC_X86_REG_ESP, sp + 4 + pop)
        self.mu.reg_write(UC_X86_REG_EIP, self.r32(sp))

    def mfc(self, ordinal):
        this = self.mu.reg_read(UC_X86_REG_ECX)
        if ordinal == ORD_STR_CTOR or ordinal == ORD_STR_DTOR:
            return self.ret(0, this)
        if ordinal == ORD_ARR_CTOR:  # CDWordArray(): vtable, data, size, max, grow
            self.calls.append(("ctor", this))
            self.mu.mem_write(this, struct.pack("<5I", 0xA77A0000, 0, 0, 0, 0))
            return self.ret(0, this)
        if ordinal == ORD_ARR_DTOR:
            self.calls.append(("dtor", this))
            return self.ret(0)
        if ordinal == ORD_SETSIZE:
            n = self.arg(0)
            self.calls.append(("setsize", this, n))
            if n and not self.r32(this + 4):
                self.w32(this + 4, self.alloc(4 * n + 64))
            self.w32(this + 8, n)
            return self.ret(8)
        if ordinal == ORD_SETATGROW:  # SetAtGrow(index, value)
            i, v = self.arg(0), self.arg(1)
            self.calls.append(("setatgrow", this, i, v))
            if not self.r32(this + 4):
                self.w32(this + 4, self.alloc(4 * 256))
            self.w32(self.r32(this + 4) + 4 * i, v)
            self.w32(this + 8, max(self.r32(this + 8), i + 1))
            return self.ret(8)
        if ordinal == ORD_NEW:  # cdecl
            return self.cdecl(self.alloc(self.arg(0)))
        if ordinal == ORD_DELETE:
            return self.cdecl(0)
        raise RuntimeError(f"unexpected MFC call, ordinal {ordinal}, from {self.r32(self.mu.reg_read(UC_X86_REG_ESP)):#x}")

    def cdecl(self, value):
        return self.ret(0, value)

    def alloc(self, n):
        a, self.top = self.top, self.top + ((n + 15) & ~15)
        return a

    def run(self, fn, this, *args):
        sp = self.STACK + 0x100000 - 0x1000
        for v in reversed((self.RET,) + args):
            sp -= 4
            self.w32(sp, v)
        self.mu.reg_write(UC_X86_REG_ESP, sp)
        self.mu.reg_write(UC_X86_REG_ECX, this)
        self.calls = []
        self.mu.emu_start(fn, self.RET, count=2_000_000)
        return self.calls, self.mu.reg_read(UC_X86_REG_EAX)

    def arrays(self, rec):
        return [list(struct.unpack(f"<{self.r32(rec + LO + 0x14 * k + 8)}I",
                                   self.mu.mem_read(self.r32(rec + LO + 0x14 * k + 4), 4 * self.r32(rec + LO + 0x14 * k + 8))))
                if self.r32(rec + LO + 0x14 * k + 8) else [] for k in range(N)]

    def fill(self, rec, items):
        for k, xs in enumerate(items):
            a = rec + LO + 0x14 * k
            data = self.alloc(4 * len(xs) + 16)
            self.mu.mem_write(data, struct.pack(f"<{len(xs)}I", *xs))
            self.mu.mem_write(a + 4, struct.pack("<4I", data, len(xs), len(xs), 0))

    def archive(self, storing, data=b""):
        ar, buf = self.alloc(0x100), self.alloc(0x10000)
        self.mu.mem_write(ar, b"\0" * 0x100)
        self.w32(ar + 0x18, 0 if storing else 1)
        self.mu.mem_write(buf, data)
        self.w32(ar + 0x28, buf)
        self.w32(ar + 0x2C, buf + (0x10000 if storing else len(data)))
        return ar, buf


def check(path):
    e = Exe(path)
    name = path.rsplit("/", 1)[-1]
    expected = EXPECTED.get(e.table)
    ok = True

    def fail(msg):
        nonlocal ok
        ok = False
        print(f"{name}: FAIL {msg}")

    if expected is None:
        fail(f"unknown exe: records at {e.table:#x}")
        return False
    roles = {r: [f for f, x in expected.items() if x == r] for r in set(expected.values())}
    if e.ctor not in roles["ctor"] or e.dtor not in roles["dtor"] or e.serialize not in roles["serialize"]:
        fail(f"found ctor {e.ctor:#x} dtor {e.dtor:#x} serialize {e.serialize:#x}, not the expected ones")
    print(f"{name}: records {e.table:#x} (3 x {STRIDE:#x}), ctor {e.ctor:#x}, dtor {e.dtor:#x}, Serialize {e.serialize:#x}")

    absolute, offsets = static_hits(e)
    if absolute:
        fail("absolute references into the array blocks: " + ", ".join(f"{i.address:#x}" for i in absolute))
    found = {}
    for i in offsets:
        found.setdefault(e.function_of(i.address), []).append(i.address)
    for f, sites in sorted(found.items()):
        print(f"  {f:#x} {expected.get(f, 'UNEXPECTED'):9} " + " ".join(f"{a:#x}" for a in sites))
    if set(found) != set(expected):
        fail(f"functions using the block offsets: {sorted(map(hex, found))}, expected {sorted(map(hex, expected))}")

    em = Emu(e)
    rec = em.alloc(STRIDE)
    arrays = [rec + LO + 0x14 * k for k in range(N)]
    # the constructor, and the MFC factory (`CreateObject`: operator new(0x338), then the same construction): ten CDWordArray
    # objects, each emptied
    for ctor in roles["ctor"] + roles["create"]:
        em.mu.mem_write(rec, b"\xcc" * STRIDE)
        calls, obj = em.run(ctor, rec)
        obj = rec if ctor in roles["ctor"] else obj
        want = [obj + LO + 0x14 * k for k in range(N)]
        made = [c[1] for c in calls if c[0] == "ctor" and c[1] in want]
        emptied = [c[1] for c in calls if c[0] == "setsize" and c[1] in want and c[2] == 0]
        if made != want or emptied != want:
            fail(f"ctor {ctor:#x}: built {len(made)}, emptied {len(emptied)} of the ten arrays")
    print(f"  ctor {', '.join(hex(c) for c in roles['ctor'] + roles['create'])}: ten arrays built, each SetSize(0, -1)")
    # the destructors destroy the ten
    for dtor in roles["dtor"]:
        calls, _ = em.run(dtor, rec, 0) if dtor != e.dtor else em.run(dtor, rec)
        if sorted(c[1] for c in calls if c[0] == "dtor" and c[1] in arrays) != arrays:
            fail(f"dtor {dtor:#x} does not destroy the ten arrays")
    # the reset at a new mission empties the ten
    items = [[(k + 1) << 16 | j for j in range(k)] for k in range(N)]
    for reset in roles["reset"]:
        em.fill(rec, items)
        em.run(reset, rec)
        if em.arrays(rec) != [[]] * N:
            fail(f"reset {reset:#x} leaves {em.arrays(rec)}")
    # ... and puts back the nine default bookmarks and the Tab / H indexes 0 (OpenSOW starts a mission with the same)
    defaults = [0x1C0, 0x640, 0x400, 0x640, 0x6C0, 0x640, 0x1C0, 0x440, 0x400, 0x440, 0x6C0, 0x440, 0x1C0, 0x240, 0x400, 0x240, 0x6C0, 0x240]
    em.mu.mem_write(rec + 0x2DC, b"\x55" * (0x32C - 0x2DC))
    em.run(roles["reset"][0], rec)
    got = struct.unpack("<20I", em.mu.mem_read(rec + 0x2DC, 80))
    if list(got[2:]) != defaults or got[:2] != (0, 0):
        fail(f"reset leaves indexes {got[:2]} and bookmarks {got[2:]}")
    print(f"  reset {', '.join(hex(c) for c in roles['reset'])}: empties all ten, default bookmarks, Tab / H index 0")
    # Serialize stores a count and the items per array and reads them back the same way
    em.fill(rec, items)
    ar, buf = em.archive(True)
    em.run(e.serialize, rec, ar)
    stream = bytes(em.mu.mem_read(buf, em.r32(ar + 0x28) - buf))
    tail = b"".join(struct.pack(f"<I{len(xs)}I", len(xs), *xs) for xs in items)
    if not stream.endswith(tail):
        fail("Serialize does not end with count + items per array")
    back = em.alloc(STRIDE)
    em.run(roles["ctor"][0], back)
    ar, _ = em.archive(False, stream)
    em.run(e.serialize, back, ar)
    if em.arrays(back) != items:
        fail(f"Serialize load gives {em.arrays(back)}")
    print(f"  Serialize {e.serialize:#x}: count + u32 items per array, the load restores them ({sum(map(len, items))} items)")
    # a record as a game has it: constructed and reset, stores ten zero counts
    em.run(roles["reset"][0], back)
    ar, buf = em.archive(True)
    em.run(e.serialize, back, ar)
    stream = bytes(em.mu.mem_read(buf, em.r32(ar + 0x28) - buf))
    if not stream.endswith(b"\0" * 4 * N):
        fail("a reset record does not store ten empty arrays")
    print("  a reset record stores ten zero counts" if ok else "")
    print(f"{name}: {'ok' if ok else 'FAILED'}")
    return ok


if __name__ == "__main__":
    sys.exit(0 if all([check(p) for p in sys.argv[1:]]) else 1)
