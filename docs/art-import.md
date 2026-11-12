# Art export / import (upscaling workflow)

Not in the exe: an OpenSOW tool for local experiments (nothing runs unless you call it).

`opensow export <dir>` writes ONE flat directory with every colour sprite frame (`.ps6`/`.tsp`, RGBA PNG, e.g.
`units__1a_blue__0008.png`), every background (`.srf` -> JPG, e.g. `levels__m01.jpg`) and `manifest.csv`
(file, frame, size, hotspot, `targets`). `targets` lists every place the identical file lives
(`prefix:a|f:path`: prefix `wm__` = Warmonger, `a` = inside `sprites.data`, `f` = loose file under `Data`),
so one edit is written to all copies. Not exported: shadows/masks (`.sha`, `.msk`, `.til`), `.srfa`, `DiffData`.

`opensow import <dir> <out> [--quality 95] [--alpha 128]` re-encodes what is in `<dir>`:
- Only files present are used; a missing frame/background keeps the original art, so a subset can be upscaled.
- A frame PNG may have any size. Alpha >= `--alpha` is opaque, RGB goes to RGB565 with rounding, the hotspot is
  scaled by new/old size, the frame's `extra` words are kept. Untouched frames re-encode bit for bit.
- A background may be the `.png` or the `.jpg` (PNG wins). PNGs are JPEG-encoded 4:4:4 at `--quality`; the
  obfuscated 55-byte SRF header is copied from the original; segments are put in the installed order (DQT, SOF, ...).
- Output is an overlay: `<out>/State of War[ Warmonger]/Data/...` with edited loose files and a rebuilt
  `sprites.info`/`sprites.data` (all original entries, edited ones replaced). Files identical to the install are skipped.
  Nothing touches the installation; back it up and copy the overlay over it.

Verified (tests/real_data.rs): `sprite::encode` and `megafile::build` reproduce every installed sprite file and the
archive byte for byte (`sprite_encoder_and_archive_builder_reproduce_the_install`); `srf::from_jpeg(to_jpeg(x))`
reproduces every `.srf`/`.srfa` (`srf_wrapper_reproduces_the_install`). Checked by hand, no test: an untouched export
imports as "0 files changed".

NOT verified: that the original game accepts other frame/background sizes. It lays things out from the original
sizes (e.g. bg SRF size == map cells*32), so upscaled art needs engine support (draw scale) - OpenSOW's own
renderer is the place for that. Backgrounds' IJL tolerance of jpeg-encoder output is untested in the original exe.
