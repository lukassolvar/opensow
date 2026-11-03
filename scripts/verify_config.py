#!/usr/bin/env python3
"""Run the exe's CPassedLevels serializer (0x456ac0, the player.cfg writer) on random level states and decode its output
with `opensow cfg` (formats::config::PassedLevels).

    /tmp/v/bin/python scripts/verify_config.py "<install>/State of War.exe" target/release/opensow
"""
import random, struct, subprocess, sys, tempfile, os
import pefile
from unicorn import UC_ARCH_X86, UC_HOOK_CODE, UC_MODE_32, Uc
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

exe_path, opensow = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path); base = pe.OPTIONAL_HEADER.ImageBase
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
for s in pe.sections:
    mu.mem_write(base + s.VirtualAddress, s.get_data())
STACK, OBJ, AR, BUF, RET = 0x70000000, 0x60000000, 0x61000000, 0x62000000, 0x50000000
mu.mem_map(STACK, 0x100000); mu.mem_map(OBJ, 0x1000); mu.mem_map(AR, 0x1000); mu.mem_map(BUF, 0x10000); mu.mem_map(RET, 0x1000)
mu.mem_write(RET, b"\xf4")
rnd = random.Random(5)

def fake_random(uc, addr, size, _):
    # 0x437cd0(n) is cdecl and reads a table the start-up code fills: return a value below n instead
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret, n = struct.unpack("<II", uc.mem_read(esp, 8))
    uc.reg_write(UC_X86_REG_EAX, rnd.randrange(n or 1))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)

mu.hook_add(UC_HOOK_CODE, fake_random, begin=0x437CD0, end=0x437CD0)
# the visual random generator 0x437cd0 reads a table the exe fills at start-up; fill it with noise
bad = 0
for case in range(20):
    states = [rnd.randrange(3) for _ in range(50)]
    mu.mem_write(OBJ, bytes(4) + bytes(states) + bytes(0x100))
    mu.mem_write(AR, b"\0" * 0x100)
    mu.mem_write(AR + 0x28, struct.pack("<I", BUF))          # write pointer
    mu.mem_write(AR + 0x2C, struct.pack("<I", BUF + 0x10000))  # end of buffer
    sp = STACK + 0x80000
    sp -= 4; mu.mem_write(sp, struct.pack("<I", AR))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp); mu.reg_write(UC_X86_REG_ECX, OBJ)
    mu.emu_start(0x456AC0, RET, count=2_000_000)
    end = struct.unpack("<I", mu.mem_read(AR + 0x28, 4))[0]
    data = bytes(mu.mem_read(BUF, end - BUF))
    with tempfile.NamedTemporaryFile(delete=False, suffix=".cfg") as f:
        f.write(data)
    out = subprocess.run([opensow, "cfg", f.name], capture_output=True, text=True)
    os.unlink(f.name)
    want = "level states [" + ", ".join(map(str, states)) + "]"
    if len(data) != 4 + 313 or out.stdout.strip() != want:
        bad += 1; print("MISMATCH", len(data), out.stdout.strip()[:80], out.stderr.strip()[:80])
print("player.cfg:", "ok" if not bad else f"{bad} mismatches")
sys.exit(1 if bad else 0)
