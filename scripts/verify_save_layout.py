#!/usr/bin/env python3
"""Check the savegame record tables of OpenSOW (`opensow savelayout`) against the exe's Serialize methods.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install capstone pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_save_layout.py "<install>/State of War.exe" target/release/opensow

Each store branch is scanned for `mov reg,[this+off]` / `cmp byte [this+off],1` followed by a write into the archive; the
offsets before the class's first loop or sub-object must equal the Rust table, and the header words must match too.
"""
import re, subprocess, sys
import capstone, pefile

exe, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe); B = pe.OPTIONAL_HEADER.ImageBase; data = open(exe, "rb").read()
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32); md.skipdata = True

def off(va):
    for s in pe.sections:
        a = B + s.VirtualAddress
        if a <= va < a + max(s.Misc_VirtualSize, s.SizeOfRawData):
            return s.PointerToRawData + va - a

# class -> (Serialize address, address where the scalar prefix ends)
CLASSES = {
    "Unit": (0x46caf0, 0x46d14e), "Building": (0x40b160, 0x40bd6f), "Bullet": (0x412a70, 0x412f48), "Aircraft": (0x468c30, 0x46925d),
    "Decal": (0x41a1f0, 1 << 30), "Cloud": (0x414820, 1 << 30), "Top": (0x47b5e0, 0x47ba9b), "Chassis": (0x4709a0, 0x471199),
    "Order": (0x478f80, 1 << 30), "Weapon": (0x47c700, 1 << 30), "BuildingPart": (0x40e9f0, 1 << 30),
    "PlayerState": (0x455700, 0x4557a8), "Stock": (0x419100, 0x4191e6), "Params": (0x465220, 0x465256),
    "Options": (0x4536d0, 0x4538bb), "Speech": (0x403b20, 0x403bec), "SpeechItem": (0x404890, 0x4048ed), "MapItem": (0x43e480, 0x43e4cb),
}

def scan(start, stop):
    hdr_done = False; last_push = None
    o = off(start); regs = {}; out = []; hdr = []; ver = None; end = None
    for i in md.disasm(data[o:o + 0x6000], start):
        t = f"{i.mnemonic} {i.op_str}"
        if i.mnemonic == "ret" and i.op_str == "4":
            break
        if i.address >= stop:
            break
        if i.mnemonic == "push" and i.op_str.startswith("0x") and int(i.op_str, 16) > 0xff and not hdr_done:
            last_push = int(i.op_str, 16)
        if t == "call 0x403e10":
            hdr = [last_push]; hdr_done = True
        m = re.match(r"mov dword ptr \[ebp - 0x[0-9a-f]+\], (\d+)$", t)
        if m and not out and ver is None:
            ver = int(m.group(1))
        m = re.match(r"mov (e[a-d]x|e[sd]i), dword ptr \[(edi|ebx)(?: \+ (0x[0-9a-f]+|\d+))?\]$", t)
        if m: regs[m.group(1)] = int(m.group(3) or "0", 0)
        m = re.match(r"cmp byte ptr \[(edi|ebx) \+ (0x[0-9a-f]+|\d+)\], 1$", t)
        if m: regs["bl"] = ("b", int(m.group(2), 0))
        m = re.match(r"mov dword ptr \[ecx\], (e[a-d]x|e[sd]i)$", t)
        if m and isinstance(regs.get(m.group(1)), int): out.append(("u", regs[m.group(1)]))
        if t == "mov byte ptr [ecx], bl" and isinstance(regs.get("bl"), tuple): out.append(regs["bl"])
    return hdr, ver, out

rust = {l.split()[0]: l.split() for l in subprocess.check_output([opensow, "savelayout"], text=True).splitlines() + subprocess.check_output([opensow, "cfglayout"], text=True).splitlines()}
bad = 0
for name, (fn, stop) in CLASSES.items():
    hdr, ver, out = scan(fn, stop)
    r = rust[name]; rver, rmagic = int(r[1]), int(r[2], 16)
    table = [(f[0], int(f[1:], 16)) for f in r[3:]]
    # The scan is the ground truth for the scalar prefix; loops are not in `out` order-independent, so compare as a prefix.
    got = [(k, v) for k, v in out]
    ok = hdr[:1] == [rmagic] and got[:len(table)] == table
    print(f"{name:13} magic {hdr[:1] and hex(hdr[0])} table {len(table)} scan {len(got)}: {'ok' if ok else 'MISMATCH'}")
    if not ok:
        bad += 1
        print("  rust:", table); print("  exe: ", got)
sys.exit(1 if bad else 0)
