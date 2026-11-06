#!/usr/bin/env python3
"""Run the exe's whole savegame *store* function (`0x42a400`, the state stream of the `.data` file) in an emulator on a randomised
game state and compare the stream with OpenSOW's `save::State` (`opensow savestate`).

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_save_state.py "<install>/State of War.exe" target/release/opensow

The globals the function reads (level, RNG index, player words and byte arrays, the three list records, the map object with its
cell layers / camera records / items, the PlayerState, Stock and Params blocks, the speech queues, the decal flag grid, the unit /
building / bullet / squad slot tables with objects built as in `verify_save_store.py`, the decal and cloud lists) are filled with
random values; MFC's CString and CArchive helpers (imports of mfc140.dll and their thunks) are replaced by small emulations.
Checked for each of 40 states:
  * `State::parse` consumes the exe's stream exactly and `State::to_bytes` reproduces it byte for byte
  * the head scalars, per-player words and byte arrays, list contents, map size / scroll / cameras / first cells, the slot counts
    and present-slot indexes, the decal flag grid (orientation included), object counts and speech texts equal the values in memory.
The 313-byte campaign signature (`0x456ac0`) is random in the exe; it is carried through unchanged.
"""
import os, struct, subprocess, sys, tempfile

from savestore_lib import *
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP

rnd2 = random.Random(77)
MAP = 0x6BF980
N = {"units": 6, "buildings": 4, "bullets": 3, "squads": 3}


def fill(addr, size):
    mu.mem_write(addr, bytes(rnd2.getrandbits(8) for _ in range(size)))


def setup(trial):
    """Randomises every global the store function reads; returns the expected summary pieces."""
    reset_heap()
    global str_top
    str_top = STR
    exp.clear()
    e = {}
    mu.mem_write(HEAP, b"\0" * 0x300000)

    def g32(a, v=None):
        v = rnd2.getrandbits(32) if v is None else v
        w32(a, v)
        return v

    e["head"] = [g32(0x49A33C), g32(0x71C0F4), g32(0x5BAAFC)]
    b1 = rnd2.choice([0, 1, 2]); mu.mem_write(0x49A339, bytes([b1]))
    e["head"].append(1 if b1 == 1 else 0)
    e["head"] += [g32(0x5BAAD8), g32(0x5BAADC), g32(0x5BAAC4), g32(0x5BAAC8)]
    b2 = rnd2.choice([0, 1, 5]); mu.mem_write(0x5BAA99, bytes([b2]))
    e["head"].append(1 if b2 == 1 else 0)
    e["head"].append(g32(0x89CF50))
    e["pw"], e["parr"] = [], []
    for p in range(3):
        e["pw"].append([g32(0x517AF8 + 4 * p), g32(0x517B04 + 4 * p), g32(0x5BAAA0 + 4 * p)])
        arrs = []
        for base_, n in ((0x5BB910, 1000), (0x5BC4C8, 300), (0x5BC850, 500), (0x5BCE30, 30)):
            fill(base_ + n * p, n)
            arrs.append(bytes(mu.mem_read(base_ + n * p, n)))
        e["parr"].append(arrs)
    e["lists"] = []
    for r in range(3):
        pair = []
        for ptr_off, cnt_off in ((0x6BF90C, 0x6BF910), (0x6BF948, 0x6BF94C)):
            n = rnd2.randrange(0, 5)
            arr = alloc(4 * max(n, 1))
            vals = [rnd2.getrandbits(32) for _ in range(n)]
            mu.mem_write(arr, struct.pack(f"<{n}I", *vals) if n else b"")
            w32(ptr_off + 0x14 * r, arr)
            w32(cnt_off + 0x14 * r, n)
            pair.append(vals)
        e["lists"].append(pair)
    # map object
    mu.mem_write(MAP, b"\0" * 0x4C0CC)
    width, height = rnd2.randrange(1, 12), rnd2.randrange(1, 12)
    w32(MAP + 0x40, width); w32(MAP + 0x44, height)
    e["scroll"] = (g32(MAP + 0x20), g32(MAP + 0x2C))
    e["size"] = (width, height)
    for off, n in ((0xCC, 0x8000), (0x80CC, 0x10000), (0x180CC, 0x10000), (0x280CC, 0x4000), (0x2C0CC, 0x4000), (0x300CC, 0x4000), (0x340CC, 0x10000)):
        fill(MAP + off, n)
    set_cstring(0x71CEC8, "VER7")
    e["cams"] = []
    for k in range(3):
        vals = [rnd2.getrandbits(32) for _ in range(6)]
        sc = rnd2.choice([0, 1, 7])
        mu.mem_write(MAP + 4 + k, bytes([sc]))
        for off, v in zip((0x8, 0x14, 0x54, 0x48, 0x60, 0x6C), vals):
            w32(MAP + off + 4 * k, v)
        e["cams"].append([1 if sc == 1 else 0] + vals)
    nitems = rnd2.randrange(0, 3)
    arr = alloc(4 * 4)
    for i in range(nitems):
        it = alloc(SIZE["MapItem"]); mu.mem_write(it, b"\0" * SIZE["MapItem"])
        w32(it, VT["MapItem"])
        kind = rnd2.choice([0, 1, 2])
        w32(it + 0xC, kind)
        for off in (4, 8, 0x10, 0x14, 0x18):
            w32(it + off, rnd2.getrandbits(32))
        mu.mem_write(it + 0x1C, bytes([rnd2.choice([0, 1]), rnd2.choice([0, 1])]))
        w32(arr + 4 * i, it)
    w32(MAP + 0xBC, arr); w32(MAP + 0xC0, nitems)
    e["items"] = nitems
    # player / stock / params blocks and the campaign object
    for p in range(3):
        for cls, base_, stride in (("PlayerState", 0x70BA50, 0xC68), ("Stock", 0x6BED08, 0x338), ("Params", 0x6BF788, 0x80)):
            a = base_ + stride * p
            mu.mem_write(a, b"\0" * stride)
            build(cls, a, f"{cls}{p}")
    fill(0x71C38C, 0x400)
    # speech queues: [0x575720] + 1 objects
    last = rnd2.randrange(0, 2)
    w32(0x575720, last)
    e["speech"] = last + 1
    e["speech_texts"] = []
    for i in range(last + 1):
        o = 0x6BF6B0 + 0x20 * i
        mu.mem_write(o, b"\0" * 0x20)
        w32(o + 4, rnd2.getrandbits(32)); w32(o + 8, rnd2.getrandbits(32))
        n = rnd2.randrange(0, 3)
        arr = alloc(0x18 * max(n, 1)); mu.mem_write(arr, b"\0" * 0x18 * max(n, 1))
        texts = []
        for j in range(n):
            it = arr + 0x18 * j
            w32(it, VT["SpeechItem"])
            t = rnd2.choice(["01-game-saved", "lost", "x" * 300, ""])
            set_cstring(it + 4, t)
            texts.append(t)
            for off in (8, 0xC, 0x10, 0x14):
                w32(it + off, rnd2.getrandbits(32))
        w32(o + 0x10, arr); w32(o + 0x14, n)
        e["speech_texts"].append("|".join(texts))
    # decal flags
    mu.mem_write(0x513878, bytes(rnd2.choice([0, 0, 1, 2]) for _ in range(128 * 128)))
    # slot groups
    e["slots"] = {}
    for name, cnt_base, tab, stride, nslots, cls in (
        ("units", 0x575734, 0x5B62C0, 0xFA0, 1000, "Unit"), ("buildings", 0x575740, 0x5BAB00, 0x4B0, 300, "Building"),
        ("bullets", 0x57574C, 0x5B91A0, 0x7D0, 500, "Bullet"), ("squads", 0x575758, 0x5BA910, 0x78, 30, "Aircraft"),
    ):
        for p in range(3):
            mu.mem_write(tab + stride * p, b"\0" * stride)
            idx = sorted(rnd2.sample(range(nslots), rnd2.randrange(0, N[name] + 1)))
            cnt = g32(cnt_base + 4 * p)
            for i in idx:
                o = alloc(SIZE[cls]); mu.mem_write(o, b"\0" * SIZE[cls])
                build(cls, o, f"{name}{p}.{i}")
                w32(tab + stride * p + 4 * i, o)
            e["slots"][(name, p)] = (cnt, idx)
    # decal and cloud lists
    for name, ptr, cnt, cls in (("decals", 0x49A3D4, 0x49A3D8, "Decal"), ("clouds", 0x49A3FC, 0x49A400, "Cloud")):
        n = rnd2.randrange(0, 4)
        arr = alloc(4 * max(n, 1))
        for i in range(n):
            o = alloc(SIZE[cls]); mu.mem_write(o, b"\0" * SIZE[cls])
            build(cls, o, f"{name}.{i}")
            w32(arr + 4 * i, o)
        w32(ptr, arr); w32(cnt, n)
        e[name] = n
    return e


def run_state():
    archive()
    sp = STACK + 0x100000 - 0x1000
    sp -= 4; mu.mem_write(sp, struct.pack("<I", ARCH))
    sp -= 4; mu.mem_write(sp, struct.pack("<I", RET))
    mu.reg_write(UC_X86_REG_ESP, sp)
    try:
        mu.emu_start(0x42A400, RET, count=200_000_000)
    except Exception as ex:
        eip = mu.reg_read(UC_X86_REG_EIP)
        sp = mu.reg_read(UC_X86_REG_ESP)
        print(f"emulation stopped: {ex} at {eip:#x}, stack {[hex(r32(sp + 4 * i)) for i in range(6)]}")
        raise
    return bytes(mu.mem_read(BUF, r32(ARCH + 0x28) - BUF))


def summary(text):
    d = {}
    lines = text.splitlines()
    d["rewrite"] = lines[0].split()[1]
    for l in lines[1:]:
        k, _, rest = l.partition(" ")
        d.setdefault(k, []).append(rest)
    return d


def main():
    bad = 0
    for trial in range(40):
        e = setup(trial)
        data = run_state()
        with tempfile.NamedTemporaryFile("w", suffix=".hex", delete=False) as f:
            f.write(data.hex())
        out = subprocess.run([opensow, "savestate", str(e["speech"]), f.name], capture_output=True, text=True)
        os.unlink(f.name)
        if out.returncode != 0:
            print(f"trial {trial}: opensow failed on {len(data)} bytes: {out.stderr.strip()[:300]}")
            bad += 1
            break
        d = summary(out.stdout)
        problems = []
        if d["rewrite"] != "ok":
            problems.append("to_bytes differs from the exe's stream")
        if [int(x) for x in d["head"][0].split()] != e["head"]:
            problems.append(f"head {d['head'][0]} vs {e['head']}")
        for p in range(3):
            if [int(x) for x in d["pw"][p].split()[1:]] != e["pw"][p]:
                problems.append(f"player words {p}")
            for k in range(4):
                got = bytes.fromhex(d["parr"][4 * p + k].split()[2]) if len(d["parr"][4 * p + k].split()) > 2 else b""
                if got != e["parr"][p][k]:
                    problems.append(f"player array {p}.{k}")
        for r in range(3):
            for k in range(2):
                parts = d["list"][2 * r + k].split()[2:]
                if [int(x) for x in parts] != e["lists"][r][k]:
                    problems.append(f"list {r}.{k}")
        m = d["map"][0].split()
        if (m[0], int(m[1]), int(m[2]), int(m[3]), int(m[4]), int(m[5])) != ("VER7", *e["scroll"], *e["size"], e["items"]):
            problems.append(f"map {m} vs {e['scroll']} {e['size']} {e['items']}")
        for k in range(3):
            if [int(x) for x in d["cam"][k].split()[1:]] != e["cams"][k]:
                problems.append(f"camera {k}")
        # cells: memory index x*128+y, stream order y outer, x inner
        w, h = e["size"]
        for i, l in enumerate(d["cell"]):
            if i >= w * h:
                break
            y, x = divmod(i, w)
            idx = x * 128 + y
            want = (r16 := struct.unpack("<H", mu.mem_read(MAP + 0xCC + 2 * idx, 2))[0], r32(MAP + 0x80CC + 4 * idx), r32(MAP + 0x180CC + 4 * idx),
                    mu.mem_read(MAP + 0x280CC + idx, 1)[0], mu.mem_read(MAP + 0x2C0CC + idx, 1)[0], mu.mem_read(MAP + 0x300CC + idx, 1)[0],
                    r32(MAP + 0x340CC + 4 * idx))
            got = tuple(int(x) for x in l.split()[1:])
            if got != tuple(want):
                problems.append(f"cell {i} {got} vs {want}")
                break
        for (name, p), (cnt, idx) in e["slots"].items():
            row = [r for r in d["slots"] if r.startswith(f"{name} {p} ")][0].split()
            if int(row[2]) != cnt or [int(x) for x in row[3:]] != idx:
                problems.append(f"slots {name} {p}: {row[2:]} vs {cnt} {idx}")
        flags = bytes.fromhex(d["decalflags"][0])
        mem = bytes(mu.mem_read(0x513878, 128 * 128))
        # stream index y*128+x holds memory index x*128+y
        want = bytes(1 if mem[x * 128 + y] == 1 else 0 for y in range(128) for x in range(128))
        if flags != want:
            problems.append("decal flag grid orientation")
        c = d["decals"][0].split()
        if int(c[0]) != e["decals"] or int(c[2]) != e["clouds"] or int(c[4]) != e["speech"] or c[6:] != ["3", "stocks", "3", "params", "3"]:
            problems.append(f"counts {c}")
        texts = [(l.split(" ", 1) + [""])[1] for l in d["speechitems"]]
        if texts != e["speech_texts"]:
            problems.append(f"speech texts {texts} vs {e['speech_texts']}")
        if problems:
            bad += 1
            print(f"trial {trial}: {len(data)} bytes: " + "; ".join(problems[:6]))
            break
        if trial % 10 == 0:
            print(f"trial {trial}: {len(data)} bytes ok")
    print(f"{40 if not bad else 'failed'} states, {bad} mismatches")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
