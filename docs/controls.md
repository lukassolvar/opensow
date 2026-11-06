# Mouse controls (decoded)

Update function `0x416880(dx, dy, left, right)` (called every frame with the movement and the two button states); pointer object fields: `[+8]` left held, `[+0xa]` left *pressed* edge, `[+0xb]` left *released* edge, `[+0x18/0x1a/0x1b]` the same for the right button, `[+0x1c]` right "still a click" (cleared when the mouse moves more than 4 px while held), `[+0x2c]` box select active, `[+0x30/0x34]` box start, `[+0x1f0]` Advancer repair mode.

- **Left button = select and command.** What a click does is chosen by the pointer type at the release (docs/cursor.md), in `0x417686..0x417e47`:
  - pointer 2 over an own unit: select it alone (`0x43ad60`); with shift/ctrl held it is toggled in the selection. Over an own building (also on the *press* edge `[+0xa]`): select the building.
  - pointer 3 (go-to, open ground with a selection): the group go-to `0x418220` to the clicked point, snapped to the cell centre, with the `click` marker (`0x418457`: `extras\click` frames 0..8, 3 ticks each, local player only) and sound 61; blocked ground (pointer 1) gives no order.
  - pointer 4 (red brackets): attack order for every selected unit (`0x469fe0(unit, target)`), sound 59; over an enemy building the same.
  - pointers 6 / 7 / 8 with an Advancer selected: repair / destroy / upgrade order (`0x46b9f0`, `0x46b8b0`, `0x46bb30`), sound 55.
- **Enemy units** cannot be clicked into the selection (`0x417a0f` tests the owner `[unit+0x44]`; only the editor flag `[0x517ae9]` skips it), but the **drag box takes them**: `0x417700..0x41776a` runs `0x46c820` over all three players' unit lists with no owner test, setting the local flag `[unit+0x48+player]`. The unit pass `0x46a420` draws the health bar for every unit with that flag, so a box dragged over enemy units shows their bars (Advancers excepted); the order loops walk the player's own list only, so they never get orders. OpenSOW: `World::select_rect`.
- **Rally point** (docs/rally.md): the button above a selected own factory's top slot icon, or the R key, arms the mode; the next left click on free ground sets the point (cell centre, marker animation, sound 68) and ends it; right click cancels.
- **Left drag** more than 16 px (`[+0x10]`/`[+0x14]` > 16, `0x4176a2`) draws the selection box; on release `0x43ae70` selects the units inside and then clears the selection flag of every Advancer (kind 40, `0x417731`): **Advancers are never box-selected**, only by clicking them. (OpenSOW also keeps nuclear vehicles, kind 46, out of groups.)
- **Right button = cancel.** Released without a drag (`0x416a17`, `0x417934`): ends placement / air-strike mode; with an Advancer selected a click on an own building toggles repair mode (`0x417964`); any other click deselects everything (`0x43ad60(0)`). It **never gives an order** (no right-click move or attack).
- **Right drag** (more than 4 px) scrolls the map by the mouse movement (`0x416bac` -> `0x457f80(dx, dy)`), the pointer stays where it was (`[+0x38]` not updated while the right button is held).
- Screen edges scroll with the scroll arrows (docs/cursor.md).

OpenSOW: `Play::left_up` (click by `Play::pointer()`), `right_down/right_drag/right_up`, tests `playing_a_mission`, `hud_clicks_reach_the_panel_not_the_map`.
