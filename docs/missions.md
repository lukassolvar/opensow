# Per-mission code: survey result (no undecoded mission scripts exist)

The campaign has no hidden per-mission scripts beyond those already decoded. Re-checked by disassembling every reference to
the mission number `[0x49a33c]` (33 sites) and the two dispatchers:

- `0x42e580` (once per frame): `mission - 1` -> case 1 (`0x42e6f5`), `-2` -> case 3 (`0x42e662`), `-5` -> case 8 (`0x42e5b2`);
  every other mission jumps straight to the `ret` at `0x42e882`. Cases 3 and 8 only drive the Rogon pods (kind 0x2e).
- `0x42ebd0`: compares the number with 7, 0xc and 0x11 only (countdown reinforcements of M07, M12, M17).
- Other sites are not gameplay: save/load serialisation (`0x41d133`, `0x42a469`), menu/map selection (`0x42583b`, `0x426947`,
  `0x422e7c` reset to -1), music/screen selection (`0x415c3b`, `0x44d7d6`, `0x442c4c`, `0x43599a`, `0x4359f3`: `0x42c260`/`0x42c320`
  calls), the radar frame (`0x454631`, `0x455059`), the M01/M06 hints (`0x430408`, `0x430795`, `0x4360b0`), M01 carryalls
  (`0x468970`), pod unit parameters for M03/M08 (`0x470308`, `0x46a0fc`, `0x418a0c`, `0x419079`, AI pick `0x42f81d`), the HQ
  Advancer replacement of M07 (`0x42e36c`), the win rule (`0x421382..0x421557`) and the tutorial (M00).

So M02, 4, 5, 6, 9-11, 13-16 and 18-22 differ from a skirmish only by level data, the destroy-everything win rule
(M04, 08, 13, 16, 21, 22), the countdown modes (header words 65-67) and, for M06, the hint box. All of this is already ported
(`World::check_outcome`, `step_mission`, `reinforcements`, `update_hint`, `step_convoy`); see docs/ai.md and docs/tutorial.md.
