# Lighting effects (soft shadows, contact shadows, explosion glow)

Not in the exe. Run-time, nothing stored; made from the original pictures and drawn with primitives the GPU replay already has (shadow blits and
coverage grids, `record::cover`), so CPU and GPU pictures stay bit-identical (`--gpu-check`: 0 differ, tainted false). Code:
`opensow_viewer::fx`; hooks in `render.rs` and `play.rs`. They only act while the world is drawn (`upscale::set_world`) and are
independent of the zoom. Switches: Options page 2 (docs/options.md, `opensow.cfg` keys `soft_shadows`, `contact_shadows`,
`explosion_glow`, default on); `--no-fx` / `OPENSOW_FX=0` force all three off.

**Differs from the exe:** the three are on by default, so the default world picture is not the exe's; switched off they draw
nothing. The GPU replay is OpenSOW's too (docs/game.md); the exe draws with its CPU blitters (`OPENSOW_CPU_DRAW=1` here).

- **Soft shadows**: every shadow blit (`blit(.., shadow)`, `blit_shadow_masked`) becomes nine blits, a 3x3 grid of screen offsets
  (step `round(0.8 * zoom)` px, at least 1) with alphas `TAP_ALPHA` (14/15) that add up to the exe's 110 inside (test: within 6 of
  255), so the shadow keeps its darkness and its edge bleeds out. The masked variant keeps the screen mask (ground units' shadows do not
  fall on buildings or foreground tiles). Not softened: the Advancers' shade stencils and the cloud shadows.
- **Contact shadows**: a faint halo hugging each building's silhouette: the building's own sprite drawn before it as a faint shade
  (alpha 3..5) at 21 screen offsets on three rings (1 + 8 + 12) (centre, 0.5 R, R with R = 4 screen px per zoom step), shifted down by 0.2 R, so it is
  darkest next to the building and fades to nothing at R (`fx::contact_taps`, hook in `render_rest`); and a soft dark ellipse
  (peak 140) under every ground unit (not the hovering Advancers) sized from the chassis frame width, left out where the screen mask
  is set like the unit itself (`fx::blob_masked`, recorded as a masked `COVER` command). History: the first versions
  drew a ridge along the footprint rectangle (`blocked_cells` bounding box), which showed as a faint square under every building and,
  with a wider feather, as a big dark smudge under turrets. A footprint is not the sprite; use the sprite's own mask.
- **Explosion glow**: an additive effect sprite (`Blit::Additive`, 64..24 000 px: explosions, muzzle flashes) also lights the
  ground: a radial coverage grid centred on the sprite's brightness-weighted centre, radius 1.1x its size, colour = its mean colour
  pulled 40 % towards warm orange, peak alpha `min(150, 300 * mean brightness)`. Clouds (big frames) and dark sprites are skipped.
  The grid is centred on the screen pixel holding that centre, so it depends only on the radius and the peak: `fx::glow_grid`
  keeps it (256 at most), and the recording stores a shared grid once (`record::cover_shared`). A flame stream draws about a
  thousand identical puffs a frame; without the sharing each built and uploaded its own grid, which at zoom 3 (1920x1080, eight
  flamers) cost 7 ms of recording and doubled the GPU replay (now 1.2 ms; test `glows_of_the_same_puff_share_one_grid`).
- **Turret shadows** (not in the exe, on by default; `turret_shadows` in `opensow.cfg`): in the original only the A.A. turret casts a shadow. The
  sentry bases' `builds\shadow\sentry01..03.sha` have only empty (0x0) frames and `units\tur_01/02/03/05_*.sha` do not exist (the
  tops' shadows are `units\<top>_<team>.sha`, present only for `2AA`; same in Warmonger). With the switch on, every turret part (object
  kinds 25..29, base and top) without shadow art gets its own silhouette drawn as a shadow, moved right and up by `turret_shadow_offset`
  (0.32 / 0.11 of its height; the original `2AA`, `1A` and `2A` shadows measure about 0.4 / 0.14, reduced a little by preference), before the sprites, through the normal shadow blit
  (so soft shadows apply). The original shadow masks hold only the part not covered by the sprite and are sheared; this is a plain
  offset copy, an approximation.
- They change pictures only: no game state is touched. Cost: about 2x the recorded commands in a busy scene (350 vs 180 at
  1280x800); recording stayed about 7 ms and the GPU replay about 2 ms longer at 1920x1080 in a scene with 2 explosions and 10 units.
- Ideas not done: bloom / blur post passes (need a framebuffer-reading pass, so they could not be bit-identical), cloud shadows on
  the ground, softening the Advancer shadows.
