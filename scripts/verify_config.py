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

# State of War_1.cfg: COptions::Serialize (0x4536d0) then the audio object's Serialize (0x407ff0) on one archive, storing mode.
# Compare the bytes with formats::config::SettingsFile (`opensow cfg` prints it back).
def store(func, obj_bytes, ar_off):
    mu.mem_write(OBJ, obj_bytes.ljust(0x100, b"\0"))
    sp = STACK + 0x80000
    sp -= 4; mu.mem_write(sp, struct.pack("<I", AR))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp); mu.reg_write(UC_X86_REG_ECX, OBJ)
    mu.emu_start(func, RET, count=2_000_000)

def skip_trim(uc, addr, size, _):
    # 0x453060 (trim the cloud list to the new detail level) needs the SEH frame and game lists: return at once
    esp = uc.reg_read(UC_X86_REG_ESP)
    (ret,) = struct.unpack("<I", uc.mem_read(esp, 4))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)

mu.hook_add(UC_HOOK_CODE, skip_trim, begin=0x453060, end=0x453060)
bad2 = 0
for case in range(30):
    flags = [rnd.randrange(2) for _ in range(4)]
    detail, sa, sb = rnd.randrange(5), rnd.randrange(11), rnd.randrange(11)
    w, h, lang = rnd.choice([640, 800, 1024, 1600, 1920]), rnd.choice([480, 600, 768, 900, 1080]), rnd.randrange(8)
    vols = [rnd.randrange(101) for _ in range(3)]
    sw = [rnd.randrange(2) for _ in range(3)]
    mu.mem_write(AR, b"\0" * 0x100)
    mu.mem_write(AR + 0x28, struct.pack("<I", BUF)); mu.mem_write(AR + 0x2C, struct.pack("<I", BUF + 0x10000))
    opt = bytearray(0x28)
    opt[8], opt[9], opt[0x10], opt[0x11] = flags
    struct.pack_into("<I", opt, 0xC, detail)
    struct.pack_into("<IIII", opt, 0x14, sa, sb, w, h)
    struct.pack_into("<I", opt, 0x24, lang)
    store(0x4536D0, bytes(opt), 0)
    # the options part is a vtable method: the object's first dword is its vtable, irrelevant for the stored fields
    aud = bytearray(0x80)
    struct.pack_into("<III", aud, 0x50, *vols)
    aud[0x5C], aud[0x5D], aud[0x5E] = sw
    store(0x407FF0, bytes(aud), 0)
    end = struct.unpack("<I", mu.mem_read(AR + 0x28, 4))[0]
    data = bytes(mu.mem_read(BUF, end - BUF))
    with tempfile.NamedTemporaryFile(delete=False, suffix=".cfg") as f:
        f.write(data)
    out = subprocess.run([opensow, "cfg", f.name], capture_output=True, text=True).stdout
    os.unlink(f.name)
    want = [f"flag_a: {str(bool(flags[0])).lower()}", f"flag_b: {str(bool(flags[1])).lower()}", f"flag_c: {str(bool(flags[2])).lower()}", f"flag_d: {str(bool(flags[3])).lower()}",
            f"detail: {detail},", f"scroll_a: {sa},", f"scroll_b: {sb},", f"width: {w},", f"height: {h},", f"language: {lang},",
            f"music_volume: {vols[0]},", f"voice_volume: {vols[1]},", f"fx_volume: {vols[2]},",
            f"music_on: {str(bool(sw[0])).lower()}", f"voice_on: {str(bool(sw[1])).lower()}", f"fx_on: {str(bool(sw[2])).lower()}"]
    if len(data) != 60 or any(x not in out for x in want):
        bad2 += 1; print("MISMATCH", len(data), out.replace("\n", " ")[:200])
print("State of War_1.cfg:", "ok" if not bad2 else f"{bad2} mismatches")
sys.exit(1 if bad or bad2 else 0)
