# State of War Warmonger: audit (2026-11-08)

Warmonger lives next to the base game in the same Steam depot (`.../depot_748041/State of War Warmonger`, own exe + `Data`). Goal: one game,
a toggle on the campaign map between the two campaigns (no separate Warmonger build). Everything below was measured on the install; the exe
facts come from disassembly of `State of War Warmonger.exe` (VA = file offset + section delta, image base 0x400000).

## It is not only missions
Same engine (a ~136 KB smaller, older build; source path `div__english__warmonger_`), same formats (`opensow check` on the Warmonger dir: 806 ok, 0 failed),
but different data in these places:

| Area | Base | Warmonger |
|---|---|---|
| Campaign missions | 22 lands + tutorial, `M01..M23` | **11 missions** `M01 M02 M03 M04 M06 M07 M11 M12 M15 M20 M22` (+ tutorial `M00`), same file names, **different content** (map, til, tmi, srf, srfa, edt, sgv briefing movie) |
| Skirmish / network maps | 8 `.net` (`01-First Spark` ...) | 6 `.net` (`00-The First Step`, `01-Goldmine Valley`, `02-Air Attacks`, `03-Winter War`, `04-Burn Them All`, `05-Ground Zero`) |
| Intro / outro / map movies | `mI mO mA mB` | own `mI mO mA mB` |
| Texts | `lang.dat` + `Language/langXX.txt` (8 languages) | **none**: English only, embedded in the exe (0x85000..0x87540: building/turret/air-unit descriptions, then 4 strings per land: `TERRITORY : nn   DIFFICULTY : d / 10`, `Coordinate: ...`, briefing, objective). No lang files. |
| Sprites | `sprites.data/.info` (377 files) | **identical** 377 files; only `menu\ingame.ps6` (the HUD frame) differs. Extra loose `Sprites2\extras\radar2.tsp` |
| Menu skin | | own `MainMenu`, `Nazov`, `CS`, `CI`, `Load`, `Options`, `Network(2)`, `Statistic`, `Adapting`, `Briefing.srf` (title "BRIEFING" baked in), `001_buttons`, `options`, `loadsave_b` tsp; no `Presents`-era Steam art (`Briefing2`, `*2`, `*Background*`, `logo`) |
| Same | | `generic.file`, `lands_blue/red/points/buttons.tsp`, all sounds (141) |
| Not present | `DiffData`, `Manuals`, `units`, Steam/SFML DLLs | |
| Settings files | `State of War_1.cfg`, `player.cfg` | `State of War.cfg` (56 bytes), `savegame\player2.cfg` |

## Campaign map (`CMenuBriefing`, text function `0x43d250`)
- Still **22 lands** on the same world map (same land sprites). Land L is mission `M<L>` as in the base game.
- Only 11 lands have text; table `0x43d6e8` (22 bytes, land -> case 0..10, 11 = "No data available"): lands **1, 2, 3, 4, 6, 7, 11, 12, 15, 20, 22** -> cases 0..10. All other lands stay locked/"No data available".
- Territories by case: 1 Crystal Brotherhood base (research 1000, difficulty 2), 2 small robot regiment, 3 Rogon convoy, 4 heavy turrets/45 min destroy all, 5 gold mines/hold 35 min, 6 hold 30 min then destroy both HQs, 7 take the base, 8 Golden Dale, 9 four HQs in 45 min, 10 islands/air forces, 11 Mr. Rahman (final).
- **Progress** (`CPassedLevels` `0x712724`, byte `+4+land`; `0x450b00` Pass, `0x450a70..` ctor, initial state at `0x450f3f`): initial lands **1, 2, 3** are 2 (open) (base: 1, 2, 9). `Pass(22)` = ending. Passing a land makes it 1, then: if land 15 passed -> land 22 opens; else if lands 1,2,3,4,6,7,11,12,20 are all passed -> land 15 opens; else the first still-locked land in the order **4, 6, 7, 11, 12, 20** opens.

## Mission rules
The mission-number global is `0x491360` (base `0x49a33c`). Both exes have 30 uses with the same comparison sets (win mode "destroy all" for 4/13/16/21, pods 3/8, countdown rewards 7/12/17, hints 1/6, tutorial 0), so the sim's per-number rules apply to Warmonger as they are. The Warmonger objectives fit them (M03 convoy, M04 destroy-all in 45 min, M07 30 min then reinforcements, M22 destroy-all final). A few comparisons differ (Warmonger has extra `cmp 4`, `cmp 120`, `cmp 250`, one set `{1,4,6,11}` where the base has `{1,2,6,9}`); **diff them before trusting the sim for M04/M11 specifics**. The base game's "air stock", tier flags and countdown values come from the level header, which the loader already handles.

## Already works
All 11 missions load and run headless in the current build when `OPENSOW_GAME_DIR` points at the Warmonger folder (`--play Mxx --ticks 600 --report --screenshot`, renders look right), so the sim, renderer, map/til/edt parsers and sprites need no changes.

## Implemented (2026-11-08)
Decision: Warmonger is a standalone expansion that adds no assets, so the port uses the base game's HUD, menus, sprites, sounds and fonts and takes only **mission data and texts** from Warmonger.
- `GameDir` (`formats/src/lib.rs`): optional `warmonger` root + `edition`. `resolve` redirects only `Data\Levels\M01..M22.*` and the ending movie `mO` to Warmonger while the Warmonger edition is selected (never falls back to the base game's same-named mission); `*.net` skirmish maps are the base game's plus Warmonger's (always listed after the base maps); `level_files()` is the edition-aware directory listing. The tutorial `M00`, the intro `mI`, `mA`/`mB` and everything else stay the base game's.
- Finding the install: `GameDir::locate` looks for a sibling folder whose name contains "warmonger" (needs `Data\Levels\M01.*` and a `*warmonger*.exe`), or `OPENSOW_WARMONGER_DIR`, or `--warmonger DIR`. There is no in-game path entry: a missing install shows an error box when the Warmonger label is clicked; a later launcher will set up all paths.
- Texts: `formats::warmonger::parse_texts` reads the 11 lands' four strings out of the exe at run time; lands outside `warmonger::LANDS` stay locked ("No data available").
- Progress: `PassedLevels::initial_warmonger` / `pass_warmonger` (exe `0x450f3f`, `0x450b00`); the progress file is `player.cfg` for the base game and `player2.cfg` for Warmonger (the exe's own name, `0x87f24`). **Saves share the slots and flag their campaign**: the extension after the stream's `Checksum` tag is `OpenSOW4` (level, edition byte, snapshot) for both campaigns; a stream without it (the original's) reads as the base game. Compatibility with saves of older builds is not a goal. Loading a slot selects its campaign first (an error message if Warmonger is not installed). The last edition is remembered in `edition.txt` of the user data folder (interactive runs only).
- Campaign map: two labels at the top (STATE OF WAR | WARMONGER) switch the edition; dev UNLOCK ALL opens only the 11 Warmonger lands. Headless: `--edition original|warmonger`, `--missing-warmonger`.
- Tests: synthetic overlay tests (`lib.rs`), `warmonger_missions_and_texts_load_through_the_edition_overlay` (real install), progress chain, lands texts and toggle, edition switch end to end. Quirk found: Warmonger M07 has two player turrets (kind 26, cells 68/69,45) on blocked cells.

## Open
- The few differing mission-number comparisons between the two exes (see Mission rules) are not traced yet.
- Warmonger's in-game strings (messages, hints) are taken from the base language files; not checked for differences.
