# Advancer actions: destroy / capture, repair, upgrade (decoded)

Code: `crates/opensow-formats/src/advancer.rs`, sim in `crates/opensow-game/src/sim.rs` (`order_action`, `step_actions`, `structure_destroyed`). Exe test: `building_hit_points_match_the_exes_constructor`.

## Rules (manual + tutorial strings)
- Select the Advancer and click a building. Enemy/neutral buildings are attacked and, when their hit points run out, **captured**; own buildings are **upgraded** (green arrow) or **repaired** (RMB toggles the mode). Turrets: own ones are repaired, enemy ones destroyed. "Advancer disengaged" = idle Advancer.
- Satellite protection (manual): some buildings/turrets cannot be taken by the Advancer. Not decoded (not in the game).

## The order (decoded)
- Network commands `CNet_HDestroy / HRepair / HUpgrade` (arrays `0x49a484 / 0x49a498 / 0x49a4ac`) come from the Advancer methods `0x46b8b0` (destroy), `0x46b9f0` (repair), `0x46bb30` (upgrade). Offline they call the top's setters `0x46e5c0 / 0x46e620 / 0x46e680`: `[top+0xf4/+0xf8]` = target cell centre px, `[top+0x100]` = target building id, `[top+0xfc]` = mode 1/2/3, `[top+0x130]` = event code 0x15 / 0x13 / 0x14. Then the normal goto (`0x4706a0`) flies there.
- The Advancer update (`0x46f210`) cancels the mode if its move goal leaves the target cell. When it sits on the target cell (`[+0x5c/+0x60]` == goal == target), the mode is set and the beam object (`[top+0x104]`, state `[top+0x144]` == 0 means idle) is ready, it re-checks the target (`0x46f3a6`): the building must still exist; **repair** needs `hp < max_hp` (`[b+0x74] < [b+0x70]`), **upgrade** needs `[b+0x144] < [b+0x140]` (active < available slots); then it spawns the beam/event with `0x47cf40` and sets the beam state to 6. Sound plays every third shot.
- Beam shots are events (object size 0x26cc, init `0x411dc0`, code `[+0x2658]`, target building id `[+0x2688]`, player `[+0x266c]`) run once by `0x43a6d0` -> `0x40f590`.

## Effects of one event (`0x40f590`, per beam shot)
- Strength `rate = [0x5baaac + player*4]` = **level header word 31 (player 0) / 32 (player 1) x 4** (`0x41d7b4`; the words are 6..15, usually 10, so rate 40). Check: `opensow level` header.
- **Destroy (0x15):** `hp -= 2*rate`, `tally[player] += 2*rate`, flag `[b+0x118] = 1`. **Repair (0x13):** `hp += 2*rate`, `tally[player] -= 2*rate` (not below 0); at full hp the Advancer's order is cleared (message "Turret/Building repaired"). **Upgrade (0x14):** `0x40e3d0`: `[b+0x148] += rate`; when it reaches the target of slot `n = [b+0x144]` (2000 ground / 4000 air) the active slot count goes up by one (sound 4) and the points reset.
- Normal bullet damage to buildings does the same bookkeeping (`0x42f2c0`): `hp -= dmg`, `[b+0x7c + attacker*4] += dmg`. So `tally[p]` (0 blue, 1 green, 2 neutral) = damage dealt by player p.

## Capture
- A building's constructor (`0x40a330`) sets max hp `[b+0x70]` by kind (jump table `0x40afb8`): **turrets 900 / 1250 / 1540 / 2100 / 2600 (kinds 25-29), every building 100-108 = 13536**, 109-113 = 200 (docs/levels.md), other = 50. It starts at **hp 1 with a pending heal `[b+0x6c]` = max** that the building update (`0x40de60`) pays out at **150 hp per tick**.
- hp <= 0 (`0x40de60` -> `0x409130`): flag `[b+0x69] = 1`, destruction state `[b+0x104] = 2`, explosion sounds (turret 0x57 / building 0x1e effect), then `0x409b60` runs the destruction animation. When it ends, the player loop `0x42dc60` calls `0x43ab40(player, index, 0)`: turrets (0x19-0x1d) and the HQ (0x64) are removed (`0x43a640`), **every other building is rebuilt for the player with the largest damage tally among those who are not the current owner** (`capture_winner`; nobody -> random). The rebuilt building keeps its kind and active-slot count, and plays sound 1. So "capturing" = destroying; the destroyer with the most damage gets it, also when ordinary bullets did it. The destroy animation length (`[b+0x108]` = 5 steps) is not applied in the game.
- The ownership message (`0x40a160`, 5 bit-fields: kind 7, id 0x20, x 8, y 8, owner 2) is the network form of the same change.

## In the game
`World::order_action` (left click on a building with an Advancer selected; `R` toggles repair vs upgrade for own buildings), beam shot every `ADVANCER_BEAM_TICKS` = 10 ticks (ESTIMATE from the beam state machine `0x47c6b0`/`0x47c5f0`: length `[+0x20]` = 10 counted down per tick), a coloured line shows the beam. Only factories can be upgraded (other buildings have no slot effects yet). Headless: `--play M01 --act 13,38 --ticks 4000 --report` (the neutral lab becomes the player's and heals to 13536).

## Not decoded
Exact beam period and the beam sprites, satellite protection, the AI's use of the Advancer, whether the x4 on `rate` is skipped in some modes (the load path at `0x41d7a3` has a variant without the shift), what `[b+0x118]/[+0x11c]` and `0x409f50` (a footprint scan for foreign Advancers every 240 ticks, only while `[b+0x118] == 1`) do in normal play, the economy effects of gold mine/research/windtrap levels.
