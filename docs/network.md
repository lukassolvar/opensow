# Multiplayer

Overview: a session is DirectPlay 4A; every frame a client sends its ten command queues, the host simulates and broadcasts its whole state (docs below: "Frame messages", "State records"). Input packages (`CNetMessage2`) carry the raw keyboard/mouse events.

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
Ten `CArray<CNet_X>` objects at `0x49a420 + 0x14*k` (vtable, data `+4`, size `+8`, max `+0xc`, grow `+0x10`): PlaceSentry `0x49a420` (filled at `0x4173a8`), ReleasePoint `0x49a434` (`0x4174be`), UnitGoto `0x49a448` (`0x418501`), UnitAttack `0x49a45c` (`0x46a048`), UnitScatter `0x49a470` (`0x418ea5`), HDestroy `0x49a484` (`0x46b900`), HRepair `0x49a498` (`0x46ba40`), HUpgrade `0x49a4ac` (`0x46bb80`), UfoStrike `0x49a4c0` (`0x41705d`), TechLevelUp `0x49a4d4` (`0x454c81`). They are the command records a client sends to the host ("Frame messages" below): a UnitGoto is 0xfac bytes = key `(x/32, y/32)` of the target block, a count and up to 1000 unit handles, merged per target block; the commands run at a tick when the exe's `0x41f020(player)` says the local side owns them.

## Frame messages (`0x4296a0`, once per frame; `formats::netsync`)
The sender is one function with two branches (both skipped when there are no remote players, `[0x575720] == 0`). Type bit 0 of the header tells them apart.

**Client -> host: the ten command queues** (`[0x575730] != 0`; `0x450de0` resets the message and sets type bit 0, `0x4527c0(7, player)` fills the 3-bit field 7; nothing is sent when all ten queues are empty). Each queue is items of a `1` bit plus fields, closed by a `0` bit, in this order (`Commands`; widths are bits, values least significant first; exe-tested by `frame_sender_writes_the_command_queues_and_snapshot_in_the_documented_order`):
| queue | item |
|---|---|
| PlaceSentry (`0x49a424`, 0x10 bytes) | x 7, y 7 (cells), tier 3, word `[+8]` 12 |
| ReleasePoint (`0x49a438`, 0xc) | x 7, y 7 (pixel / 32 truncating toward zero), word 32 |
| UnitGoto (`0x49a44c`, 0xfac) | x 7, y 7, count 9, that many handles (32) |
| UnitAttack (`0x49a460`, 0xfa8) | target handle 32, count 9, handles |
| UnitScatter (`0x49a474`, 0xfa4) | count 9, handles |
| HDestroy / HRepair / HUpgrade (`0x49a488`, `0x49a49c`, `0x49a4b0`) | Advancer handle 32, building handle 32 |
| UfoStrike (`0x49a4c4`, 0xc) | x 14, y 14, force code 8 |
| TechLevelUp (`0x49a4d8`) | building handle 32, slot 3 |
The queues are the local lockstep command records of the section above; the client does **not** run them itself, the host does.

**Host -> clients: the whole state, every frame** (`[0x575730] == 0`, networked): `0x450b70(a, b)` clears type bit 0 and stores the game clock `[0x70ef94]` in the 18-bit field 1 and `[0x70ef98]` in the 6-bit field 2; `0x4527c0(7, 0)`; then the payload and, after each part, its count in a header field: every building that is not destroyed (`[+0x69] != 1`) through `0x40a2d0` (count -> field 4, 9 bits), every unit with `[+0x2c] == 0` through `0x46c5c0` (field 3, 12 bits), every aircraft (`0x4688d0`, field 6, 6 bits) and every bullet that `0x4128f0` accepts (not types 22, 32, 33; field 5, 9 bits; a bullet is sent once, `[+0x26bc]` marks it), `0x403580` (a no-op), `0x4514d0` sends. So multiplayer is **host-authoritative state replication plus client command forwarding**, not a pure lockstep of inputs: the clients show what the host simulates, and the unit/building records are small (a unit is 55..90 bits). `Snapshot` writes and parses a whole message; `opensow netmsg <hex> <bits>` decodes a captured one.

## State records (`netsync::BuildingSync`, `BulletSync`, `AirSync`, `UnitSync`)
Writers `0x40a2d0`, `0x4128f0`, `0x4688d0`, `0x46c5c0` (readers `0x40a160`, `0x412620`, `0x468790`, `0x46c300` mirror them). `scripts/verify_net.py` runs the exe's own writers on 150 random objects each (emulator) and compares them with `opensow netsync <kind> <hex> <bits>`: all identical.
- building: kind `[+0x94]` 7, `[+0x90]` 32, cell x `[+4]` 8, y `[+8]` 8, owner `[+0x8c]` 2;
- bullet: type 6, owner 2, four 12-bit coordinates `[+0x269c..0x26a8]`, `[+0x2698]` 16, strength 16, `[+0x26b0]` 32, `[+0x26b4]` 6, then **one extra by type** (byte table `0x412a48`: types 1, 10, 19, 20, 21, 31 the 32-bit target handle `[+0x2688]`; type 2 the 5-bit delay `[+0x26ac]`; types 13, 14 the 8-bit speed; types 15, 16 the 7-bit variant `[+0x2680]`; type 18 the 2-bit variant `[+0x268c]`; all others none) and for types 19 to 21 a flag bit `[+0x26c0]`;
- aircraft: code 8, handle 16 `[+0x1c]`, target x/y 16 each, `[+0x98]` 16, `[+0x9c]` 16, `[+0x20]` 2, `[+0xa0]` 32, `[+0xa4]` 6 (the old note had 16 bits for the last one);
- unit: kind 16, handle 32, player 2, cell x/y 7 each (the chassis' `[+0x64]`, `[+0x68]`, or the position / 32 without a chassis), `[+0xc]`, `[+0x10]` 7 each; a chassis: bit 1, four 2-bit relations (0 equal, 1 less, 2 greater) between the chassis words `+0x64/+0x6c`, `+0x68/+0x70`, `+0x6c/+0x74`, `+0x70/+0x78`, goal cell `[+0x5c] >> 5`, `[+0x60] >> 5` 7 each, `[+0x168]` 32, `[+0x16c]` 6, a bit `[+0xe0] == 2`; no chassis: a single 0 bit; a top: bit 1, facing 5 bits (`[top+0x70]`), for the turret tops 109..113 also `[unit+0x68]` 32 bits; no top: a 0 bit.
- Not ported: the readers' effects on live objects (they create and update units through the constructors), and `0x449b70` / `0x4493e0` (lobby/setup data, docs/multiplayer-menus.md).
