# Executable analysis

- `State of War.exe`: PE32 x86 GUI, ImageBase 0x400000, entry 0x47e7bc, timestamp 2019-04-03, .text 0x1000..0x84000 (~530 KB code).
- Warmonger exe is ~100 KB smaller; same engine.
- Imports: DDRAW `DirectDrawCreate`; DINPUT `DirectInputCreateA`; DSOUND ordinal #1 (DirectSoundCreate); WINMM mmio* + timeGetTime;
  ijl15.dll ordinals 2,3,4 (Intel JPEG Library init/read/free); steam_api `SteamAPI_Init`/`RestartAppIfNecessary`; mfc140.dll (285 imports).
- SFML 2 network DLLs ship next to the exe but are NOT in the import table (maybe loaded dynamically; unverified).
- Strings reveal: `Data\lang.dat`, `savegame\player.cfg`, `State of War_1.cfg`, levels `m01..m23.sgv`, `mA/mB.sgv`, `data\levels\*.net` (multiplayer maps),
  font masks `extras\fonts\{big,medium,msg,small}_{i,o}.msk`, "Wrong map version!!!" (map tag VER7).
- ~100 RTTI classes: CArray<T> templates, CBuilding, CBullet, CCloud, CControl, CDialog, CEditorScript, CGameFrame, CHuffcomp, CInfoBox,
  CMegaFile, CMenu*, CNetConnection/Session/Player, CNet_* commands, CPlayMapObject, CUnitChassis, CUnitFoot, Crater, Smoke, AnimSprite.
- Next step: load in Ghidra (x86 32-bit, MSVC RTTI analyzer) and recover vtables from the RTTI names.

## Addresses (State of War.exe, ImageBase 0x400000)
- `0x41da00` script (.edt/.net) loader: tags 0x7b objects, 0xe0 markers, footer 0x1de8; calls `0x43a420` for each marker.
- `0x43a420` creates a unit in a per-player array (allocation 0xa0 bytes) -> ctor `0x46bd30(cellx, celly, player, slot, kind, ...)`.
- `0x46bd30` unit ctor: `kind-1` indexes HP via byte table `0x46c280`/jump table `0x46c23c`; "Unknown unit" error for others. Unit kinds: 1-15, 40-46, 109-113.
- `0x46ff50` weapon init for kinds 1-46 (byte table `0x470408`, jump table `0x4703b4`); field `+0x1c` = weapon type 0..5.
- `0x43a9b0` per-category synchronised random slot allocator (lockstep netplay); categories 1,3,4,5,6.
- Building objects: kind in `[obj+0x94]`; kinds 101,102,103,108 are unit producers (`0x4167a1`, `0x455daa`).
- Unit sprite load order (string table near file offset 0x8bc26): `1AA 1A 1C 1F 1S 2AA.. 3AA.. 1I tur_01 tur_02 tur_03 tur_05 5W1..5W5`, then chassis `chA chB chC chI1 chW chN`.
- Helper scripts used (rewrite as needed; Python venv with capstone/pefile): string/xref scan, jump-table decode, annotated disassembly.
- `0x46ff50` is the CHASSIS init (not weapon): `[obj+0x1c]` = chassis type, `[obj+0x40]` frame; draw `0x46e8f0`. Weapon top ctor: byte table `0x47ac70`, jump table `0x47ac04`,
  `[obj+0x54]` = top sprite index, draw `0x479a80`. Sprite arrays: tops blue `0x575770` / green `0x57b530` (150 slots per top), chassis blue `0x593b30` / green `0x59b830` (200 per chassis).
- Name lists are MFC CStringArrays filled at `0x423ecd` / `0x424563` (tops, in the load order above) and `0x424175` (chassis); loop at `0x424070` loads `_blue`, `_gren`, `.po1`, `.po2`.
- Method that worked: find who writes a field (jump-table scan) and who reads it in the draw routine, then join the two through the field offset.
- `0x436230` range ring list builder (offsets `0x65bbd8`, index `0x6b3a18`, neighbours `0x6b3c70`); consumer `0x47b140` (ground target search). Ported in `rings.rs`, emulation-verified.
- Aircraft: objects in `0x5ba910` (3 sides x 30); ctor `0x467ee0`, update `0x467b50` (+ per-type `0x467450` carryall, `0x467600` fighter), death `0x467850`, spawns `0x468240`, carryall squad `0x468960` (calls unit creation `0x43a420`), draw `0x467240`. Bullet ctor `0x411dc0(type, strength, x1, y1, x2, y2, side)`. Ported in `aircraft.rs`.

## Game clock (decoded)
- `0x42e890` runs once per rendered frame: `GetTickCount` fps counter, then accumulator `[0x5ddfc0]` += elapsed*100 (`0x437500` = `timeGetTime` delta in ms x 100, scaled by float `[0x48e8dc]` = 1.0). While the accumulator exceeds `0x682` (1666) it emits a tick and subtracts 1666: **one tick = 16.66 ms = 60 ticks/s**; ticks per frame go to `[0x552494]`, the in-second tick index `[0x5ddfc4]` wraps at 60 (stored per tick at `0x517b10`), the second counter is `[0x70ef94]`.
- With `[0x517aea]` != 0 (fixed-step mode, probably replay/network) the frame adds a constant 425 (1700 x 0.25) instead of measured time; not traced further.
- The main message loop is `0x41a913`: `PeekMessage`, and when idle `0x42d020` (state dispatcher on `[0x5baa8c]`, jump table `0x42d624`, 30 states), so the game runs flat out and the accumulator makes it frame-rate independent.
