# Multiplayer

The menu screens (connection, sessions, lobby, the lobby packet) are in docs/multiplayer-menus.md.

Code: `crates/opensow-formats/src/net.rs`; exe checks: `scripts/verify_net.py` (emulates the exe's own writers and readers, 200 random packages, header selectors). The exe imports no socket library: sessions use **DirectPlay 4A** (`IDirectPlay4A` through COM, strings at `0x48fbe8..0x48fd78`: "State of War session", "Player1/Player2", "Can't send network message"). Maps for it are `data\levels\*.net` (docs/levels.md). The `sfml-network` DLLs in the install are not imported.

## Bit streams
Both packet classes store bits LSB-first inside bytes (bit `p` = bit `p&7` of byte `p>>3`). `0x4528f0(value, n)` writes a number least significant bit first, `0x451200(n)` reads one; the input records below write their numbers most significant bit first.

## Transport message (`CNetMessage`, 0x4e30 bytes, buffer at `+0xc`, write position `+0x4e2c`, read position `+0x4e30`)
Header fields (selector -> bit, width; writer `0x4527c0`, reader `0x4510d0`): 0 type 4 bits @0; 7 field of 3 bits @4; 1 18 bits @7 (sender id, stored at `+4` by the receiver); 2 6 bits @0x19 (player slot, `+8`); 3 12 bits @0x1f; 4 9 bits @0x2b; 5 9 bits @0x34; 6 6 bits @0x3d; payload from bit `0x43`. The receiver `0x451390` calls `IDirectPlay4::Receive` (buffer 0x4e20 bytes), reads type, sender and slot, and returns 1 (too large), 3 (nothing), 2 (message) or 0. The sender `0x4514d0` sends `bits/8 + 1` bytes with `IDirectPlay4::Send`. `net::Header`.

## Input packages (`CNetMessage2`, 0x3f4 bytes per player at `0x70dfb8`; lockstep input)
Built every tick by `0x420b80` from DirectInput: 16 reserved bits (zero), then one record per keyboard event, then one mouse record:
- key (`0x451540`, 11 bits): `01`, type bit (1 = pressed, 0 = released), scan code 8 bits MSB first;
- mouse (`0x451920`, 36 bits): `10`, two button bits, x and y as 16-bit words MSB first (signed on read, `0x4500d0`).
The readers are `0x44fe00` (key) and `0x4500d0` (mouse), called from `0x415920` / `0x415a00` with the read position at bit 16. `net::InputPackage` parses and writes it; `opensow netpkg <hex> <bits>` decodes one. The game turns the inputs of all players into identical commands at the same tick, which is why the sim must be deterministic (`rng.rs`).

## `CNet_*` command queues
Ten `CArray<CNet_X>` objects at `0x49a420 + 0x14*k` (vtable, data `+4`, size `+8`, max `+0xc`, grow `+0x10`): PlaceSentry `0x49a420` (filled at `0x4173a8`), ReleasePoint `0x49a434` (`0x4174be`), UnitGoto `0x49a448` (`0x418501`), UnitAttack `0x49a45c` (`0x46a048`), UnitScatter `0x49a470` (`0x418ea5`), HDestroy `0x49a484` (`0x46b900`), HRepair `0x49a498` (`0x46ba40`), HUpgrade `0x49a4ac` (`0x46bb80`), UfoStrike `0x49a4c0` (`0x41705d`), TechLevelUp `0x49a4d4` (`0x454c81`). They are local lockstep command records (not sent as such): a UnitGoto is 0xfac bytes = key `(x/32, y/32)` of the target block, a count and up to 1000 unit handles, merged per target block; the commands run at a tick when the exe's `0x41f020(player)` says the local side owns them.

## State sync writers (not ported)
Bit-field widths read from the `0x4528f0` / `0x451200` call sequences (`n` bits LSB first), used to bring a joining player up to date:
- `0x40a2d0` / reader `0x40a160` (a building): 7 `[+0x94]`, 32 `[+0x90]`, 8 `[+4]`, 8 `[+8]`, 2 `[+0x8c]`;
- `0x4128f0` / `0x412620` (a bullet): 6 `[+0x2658]`, 2 `[+0x266c]`, 12 x4 `[+0x269c..0x26a8]`, 16 `[+0x2698]`, 16 `[+0x2660]`, 32 `[+0x26b0]`, 6 `[+0x26b4]`, 32 `[+0x2688]`, then a flag bit;
- `0x4688d0` / `0x468790` (an air squad): 8 `[+0x28]`, 16 `[+0x1c]`, 16 `[+0xc]`, 16 `[+0x10]`, 16 `[+0x98]`, 16 `[+0x9c]`, 2 `[+0x20]`, 32 `[+0xa0]`, 16 `[+0xa4]`;
- `0x46c5c0` / `0x46c300` (a unit): 16 `[+0x40]`, 32 `[+0x3c]`, 2 `[+0x44]`, 7 `[+0xc]`, 7 `[+0x10]`, ...;
- `0x4296a0` is the whole-game sender (units, buildings, bullets in slot loops with a present bit, like the savegame, docs/savegame.md); `0x449b70` / `0x4493e0` carry the lobby/setup data (player colours, names, map slots: 8-bit values and 16-bit pairs from `0x71cac0..`).
