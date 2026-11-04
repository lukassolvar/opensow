#!/usr/bin/env python3
"""Checks the HUD layout constants used by crates/opensow-game/src/hud.rs against the exe's code (docs/hud.md).

usage: python3 scripts/verify_hud_layout.py "State of War.exe"   (venv with capstone, pefile)
"""
import sys, pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

pe = pefile.PE(sys.argv[1]); base = pe.OPTIONAL_HEADER.ImageBase
data = open(sys.argv[1], 'rb').read()
text = [s for s in pe.sections if s.Name.startswith(b'.text')][0]
tb, toff = base + text.VirtualAddress, text.PointerToRawData
md = Cs(CS_ARCH_X86, CS_MODE_32); md.skipdata = True

def ins(va, n):
    off = toff + va - tb
    return [f"{i.mnemonic} {i.op_str}" for i in md.disasm(data[off:off + n], va)]

def has(va, n, *wanted):
    got = ins(va, n)
    for w in wanted:
        assert w in got, f"{hex(va)}: missing {w!r}"
    print("ok", hex(va), wanted)

# counters: credits drawn at (3,3), research at (0x8f,3) when credits show, else (3,3); digit offsets 0x48 / 0x57 / step 0xa / 9
has(0x4545cf, 12, "push 3", "call 0x430920")
has(0x4545e2, 20, "push 3", "push 0x8f", "call 0x430b50")
has(0x430a1d, 8, "add edi, 0x48", "add ebx, 2")
has(0x430c4c, 8, "lea esi, [edi + 0x57]")
has(0x430ca8, 3, "add esi, 9")
# radar: x = -11 - mapW (anchored right), y = -1, inner offset +6, small frame list
has(0x455004, 16, "mov eax, 0xfffffff5", "sub eax, dword ptr [0x71cec0]", "push -1")
has(0x454627, 12, "add eax, 6", "add ecx, 6")
# turret tabs: first tab at x=-11, step -0x39 (57), y=-0x15 (21), five tabs
has(0x455415, 20, "mov ebx, 0xfffffff5", "push -0x15")
has(0x455458, 24, "sub ebx, 0x39", "cmp ebx, 0xfffffed8")
# air pills: y = 3 * 0x33 * n + 0x3c, x = 0x1f (31)
has(0x453f30, 20, "imul esi, edx, 0x33", "add esi, 0x3c")
has(0x453f66, 20, "mov dword ptr [eax + edi + 0x488], 0x1f")
has(0x4551a6, 5, "mov ebx, 5")
# tab availability: credits thresholds 200..1000, flags at +0xa5..+0xa9
has(0x454e9b, 24, "cmp dword ptr [eax*4 + 0x517af8], 0xc8", "cmp byte ptr [esi + 0xa5], 0")
has(0x454f10, 40, "cmp dword ptr [eax*4 + 0x517af8], 0x3e8", "cmp byte ptr [esi + 0xa9], 0")
# header block bytes copied to the per-player flags
has(0x41d7f2, 30, "movzx eax, byte ptr [ecx + 0x5179c4]", "mov byte ptr [ecx + 0x70baf5], al", "mov byte ptr [ecx + 0x70c75d], al")
# countdown: drawn at (W/2, 13) while the header flag [0x5baacc] is set; message entries: y = -0x2a, life 0x1e0, +0x14 px push,
# text colour (0xff,0xf3,0x94) with shadow (0x4a,4,0)
has(0x4209c4, 20, "push 0xd", "sar eax, 1", "call 0x430d50")
has(0x4209a9, 8, "cmp dword ptr [0x5baacc], 0")
has(0x403311, 12, "push 0x1e0", "push -0x2a")
has(0x4032b5, 8, "add dword ptr [eax + edx + 0x14], 0x14")
has(0x403737, 40, "push 4", "push 0x4a", "push 0x94", "push 0xf3", "push 0xff")
# info box: slides between y=-91 and y=99 at x=-65 (speed 13), text lines at (X-54, Y+13), +22, +31 (x+8), +40, +49, slot column
has(0x455b6c, 40, "mov dword ptr [edi + 0x8ac], 0xffffffbf", "mov dword ptr [edi + 0x8b0], 0x63", "mov dword ptr [edi + 0x8b4], 0xd")
has(0x455bae, 30, "mov dword ptr [edi + 0x8b0], 0xffffffa5")
has(0x43b5de, 12, "lea edi, [ebx - 0x36]")
has(0x43b5ea, 4, "add ebx, 0xd")
has(0x43b620, 4, "lea eax, [ebx + 9]")
has(0x43b636, 4, "lea esi, [ebx + 0x12]")
has(0x43b655, 4, "add edi, 0x3e")
has(0x43b674, 4, "lea esi, [ebx + 0x1b]")
has(0x43b6c3, 4, "lea eax, [ebx + 0x24]")
has(0x455d09, 4, "lea ecx, [esi - 0x1c]")
has(0x455d31, 4, "add eax, 0x32")
has(0x43e077, 4, "push 0x33")
has(0x43e0b0, 4, "push 0x34")
has(0x43cc1d, 20, "mov dword ptr [esi + 0x24], 3")
# slot arrow: centre (W-68, y-3) = icon x - 0x28, y - 3; click rects (icon -27..28 x -30..27, arrow -16..15 x -17..21)
has(0x455d5a, 8, "lea eax, [edx - 0x28]")
has(0x455d75, 8, "lea eax, [esi - 3]")
has(0x455153, 12, "push 0x1b", "push 0x1c", "push -0x1e", "push -0x1b")
has(0x455180, 12, "push 0xf", "push -0x11", "push -0x10")
# slot bar `0x437390`: 4 rows, 33 px (0x21), 565 colour sets for the filled (flag 0) and the remainder (flag 1)
has(0x4373a5, 24, "mov edx, 0xa534", "mov eax, 0xd7f4", "cmovne eax, edx")
has(0x4373b4, 20, "mov edx, 0x632c")
has(0x4373bc, 8, "mov eax, 0x960c")
has(0x4373c4, 8, "mov edx, 0x528a")
has(0x4373cf, 8, "mov eax, 0x6467")
has(0x4373d7, 8, "mov edx, 0x39c7")
has(0x4373e2, 8, "mov eax, 0x3ae3")
has(0x4374a5, 8, "add ecx, 0x21")
has(0x454425, 8, "call 0x45f1c0")
has(0x45432e, 12, "add eax, 0x15", "sub eax, 8")
has(0x454305, 20, "cmp byte ptr [ebp - 1], 0", "mov ecx, dword ptr [esi + 0x148]")
# widget draw modes: 0 plain 0x45edf0, 1 additive 0x45cde0, 2 half-additive 0x45c220; overlay [+0x34] always additive
has(0x456192, 8, "call 0x45edf0")
has(0x456174, 8, "call 0x45cde0")
has(0x456156, 8, "call 0x45c220")
# locked-slot page: mode 3 set-up and the line 0x11b ("No Info Available"), char period [+0x38] = 3
has(0x43e12a, 8, "mov dword ptr [esi + 0x38], 3")
has(0x43dea3, 8, "push 0x11b")
# black radar toggled on the right button's release over the radar; the plate frames 4/5 are plain-copied
has(0x416e94, 8, "cmp byte ptr [esi + 0x1b], 1")
has(0x416ead, 20, "xor eax, eax", "sete al")
has(0x4546a8, 8, "push dword ptr [esi*4 + 0x5e60b0]")
has(0x4546c3, 8, "call 0x45edf0")
# message push: speed row 0x552ba8 indexed by the tick counter
has(0x4047e0, 20, "mov eax, dword ptr [edx*4 + 0x517b10]", "movzx eax, byte ptr [eax + 0x552ba8]")
print("all HUD layout constants match")
