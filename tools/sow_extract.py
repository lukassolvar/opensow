#!/usr/bin/env python3
"""State of War asset extractor (findings from format analysis).

Usage:
  sow_extract.py srf  <file.srf|.srfa> <out.jpg>   # SRF = obfuscated header + baseline JPEG
  sow_extract.py mega <sprites.info> <sprites.data> <out_dir>   # unpack CMegaFile archive
  sow_extract.py sprite <file.ps6|.tsp|.sha> <out.png> [cols]    # render all frames as a sheet

Sprite files (.ps6 / .tsp / .sha) are a sequence of frames until EOF:
  u32  size            bytes that follow (not counting this field)
  u16  w, h            frame size
  i16  hot_x, hot_y    anchor point
  u16  4 more values   (canvas size/centre? e.g. 640,480,320,240) - meaning unconfirmed
  u32  row_off[h]      in u16 words, relative to the end of the size field
  rows: u16 nruns, u16 first_opaque, then nruns runs alternating
        transparent/opaque, each u16 len; opaque runs in .ps6/.tsp are
        followed by len RGB565 pixels, in .sha they carry none (shadow mask).
"""
import os, struct, sys, zlib

def srf_to_jpg(src, dst):
    d = open(src, 'rb').read()
    # The first ~0x37 bytes replace the JPEG SOI/APP0 header; real data starts at the DQT marker.
    i = d.index(b'\xff\xdb')
    open(dst, 'wb').write(b'\xff\xd8' + d[i:])

def read_mega_index(info_path):
    info = open(info_path, 'rb').read()
    count = struct.unpack_from('<I', info, 4)[0]
    o, entries = 8, []
    for _ in range(count):
        e = info.index(b'\0', o)
        # Names are stored with every byte shifted by +10.
        name = bytes((b - 10) & 0xff for b in info[o:e]).decode('latin1')
        off, size = struct.unpack_from('<II', info, e + 1)
        entries.append((name, off, size))
        o = e + 9
    return entries

def unpack_mega(info_path, data_path, out_dir):
    with open(data_path, 'rb') as f:
        for name, off, size in read_mega_index(info_path):
            dst = os.path.join(out_dir, *name.split('\\'))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            f.seek(off)
            open(dst, 'wb').write(f.read(size))
            print(f'{off:10d} {size:9d} {name}')

def read_sprite(path):
    d = open(path, 'rb').read()
    shadow = path.lower().endswith(('.sha', '.msk'))
    frames, o = [], 0
    while o + 4 <= len(d):
        size = struct.unpack_from('<I', d, o)[0]
        if size == 0:
            break
        base = o + 4
        w, h, hx, hy = struct.unpack_from('<HHhh', d, base)
        extra = struct.unpack_from('<4H', d, base + 8)
        offs = struct.unpack_from('<%dI' % h, d, base + 16)
        px = [[None] * w for _ in range(h)]  # None = transparent, int = RGB565, -1 = shadow
        for y, ro in enumerate(offs):
            q = base + ro * 2
            nruns, opaque = struct.unpack_from('<HH', d, q)
            q += 4
            x = 0
            for _ in range(nruns):
                n = struct.unpack_from('<H', d, q)[0]
                q += 2
                if opaque:
                    if shadow:
                        px[y][x:x + n] = [-1] * n
                    else:
                        px[y][x:x + n] = struct.unpack_from('<%dH' % n, d, q)
                        q += 2 * n
                x += n
                opaque ^= 1
        frames.append(dict(w=w, h=h, hot=(hx, hy), extra=extra, px=px))
        o = base + size
    return frames

def rgb565(c):
    r, g, b = c >> 11, (c >> 5) & 63, c & 31
    return (r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255)

def write_png(path, w, h, rgba_rows):
    raw = b''.join(b'\0' + bytes(row) for row in rgba_rows)
    chunk = lambda t, data: struct.pack('>I', len(data)) + t + data + struct.pack('>I', zlib.crc32(t + data))
    open(path, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
                           + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))

def sprite_to_png(src, dst, cols=None):
    frames = read_sprite(src)
    cols = cols or min(len(frames), 16)
    cw, ch = max(f['w'] for f in frames) + 1, max(f['h'] for f in frames) + 1
    rows_n = (len(frames) + cols - 1) // cols
    W, H = cols * cw, rows_n * ch
    img = [bytearray(W * 4) for _ in range(H)]
    for i, f in enumerate(frames):
        ox, oy = (i % cols) * cw, (i // cols) * ch
        for y, row in enumerate(f['px']):
            for x, c in enumerate(row):
                if c is not None:
                    img[oy + y][(ox + x) * 4:(ox + x) * 4 + 4] = bytes((0, 0, 0, 128) if c == -1 else rgb565(c))
    write_png(dst, W, H, img)
    f = frames[0]
    print(f'{len(frames)} frames, first {f["w"]}x{f["h"]} hot={f["hot"]} extra={f["extra"]}')

if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'srf' and len(sys.argv) == 4:
        srf_to_jpg(sys.argv[2], sys.argv[3])
    elif cmd == 'mega' and len(sys.argv) == 5:
        unpack_mega(*sys.argv[2:5])
    elif cmd == 'sprite' and len(sys.argv) in (4, 5):
        sprite_to_png(sys.argv[2], sys.argv[3], int(sys.argv[4]) if len(sys.argv) == 5 else None)
    else:
        sys.exit(__doc__)
