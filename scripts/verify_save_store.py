#!/usr/bin/env python3
"""Run the exe's own *store* branch of every savegame object class in an emulator and compare the bytes with OpenSOW's record
reader / writer (`formats::saveobj`, `opensow saverec`).

    python3 -m venv /tmp/v && /tmp/v/bin/pip install unicorn pefile
    cargo build --release -p opensow-tools
    /tmp/v/bin/python scripts/verify_save_store.py "<install>/State of War.exe" target/release/opensow

For each class a random object is built in emulated memory (vtable pointers found in the image: the vtable whose slot 2 is the
class's Serialize), the exe's Serialize runs against an in-memory CArchive (flags +0x18 = 0 = storing, cursor +0x28, end +0x2c),
and the produced bytes are given to `opensow saverec`, which parses them, re-writes them and lists every field with its path and
in-memory offset. The check is exact:
  * the parser consumes every byte and `write(parse(x)) == x`
  * every parsed (path, offset) holds the value that was in the object's memory at that offset (so the offset tables, the
    list/route/child structure and the bool normalisation `byte == 1` are right, not just the order)
  * nothing is missing: every field the script put in memory at a serialised offset appears.
The whole state stream (`0x42a400`) is checked by `verify_save_state.py`.
"""
import os, subprocess, sys, tempfile

from savestore_lib import *

bad = 0
total = 0
for cls in ["Order", "Weapon", "BuildingPart", "Decal", "Cloud", "Aircraft", "PlayerState", "Params", "Stock", "Top", "Chassis",
            "Unit", "Building", "Bullet"]:
    n_ok = 0
    for trial in range(60):
        reset_heap()
        mu.mem_write(HEAP, b"\0" * 0x300000)
        exp.clear()
        obj = alloc(SIZE[cls])
        build(cls, obj)
        data = run(cls, obj)
        with tempfile.NamedTemporaryFile("w", suffix=".hex", delete=False) as f:
            f.write(data.hex())
        out = subprocess.run([opensow, "saverec", cls, f.name], capture_output=True, text=True)
        os.unlink(f.name)
        total += 1
        if out.returncode != 0:
            print(f"{cls}: opensow failed: {out.stderr.strip()[:200]}")
            bad += 1
            continue
        lines = out.stdout.splitlines()
        head = lines[0].split()
        got = {}
        for l in lines[1:]:
            p, o, v = l.split("|")
            got[(p, int(o))] = int(v)
        ok = head[1] == str(len(data)) and head[3] == str(len(data)) and head[5] == "ok"
        # the Rust record may carry fields the memory image did not fill (counts it derives); compare on the exp keys and on extras
        missing = [k for k in exp if k not in got]
        wrong = [(k, exp[k], got[k]) for k in exp if k in got and got[k] != exp[k]]
        extra = [k for k in got if k not in exp]
        if not ok or missing or wrong or extra:
            bad += 1
            print(f"{cls}: MISMATCH header {lines[0]} missing {missing[:5]} wrong {wrong[:5]} extra {extra[:5]}")
            break
        n_ok += 1
    print(f"{cls:13} {n_ok}/60 random objects: store bytes match the Rust reader/writer and the memory fields")
print(f"{total} objects, {bad} mismatches")
sys.exit(1 if bad else 0)
