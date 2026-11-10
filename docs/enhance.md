# Run-time texture enhancement

Nothing derived is stored or shipped: when the camera zooms in (`view.zoom > 1`), the world is drawn from 2x copies of the
original art made in memory from the user's own files (`opensow_viewer::upscale`, no new dependencies).

- **Sprites** (`Frame` has a lazy `enhanced` slot): colour is dilated into the transparent border (no dark halo), resampled with
  Lanczos3 (+ a light unsharp mask), the 1-bit mask is resampled and cut at 0.5, so edges stay a strict on/off mask like the exe's.
  Frames over 24 000 px (clouds, big blasts) are left alone. Above zoom 2 the 2x copy is enhanced again (4x).
- **Ground** (`Scene::ground`): the level picture is Lanczos3 2x with unsharp 0.5, the foreground (tree tops) mask is resampled
  the same way. It is made on a background thread the first time the camera is zoomed (about 0.25 s for 2048x2048); until it is
  ready the exe's picture is drawn. Maps over 4.3M px stay unenhanced (memory).
- **How it plugs in:** `render::hires` swaps a frame for its 2x copy and the view for (x*2, y*2, zoom/2) at the top of every
  world blit (`blit_clip`, `blit_masked`, `blit_blend_masked` except the lens, `blit_shadow_masked`, `stamp_frame`); doubling is
  exact in f32, so screen rectangles equal the original's. The same recursion covers the GPU recording. `Scene::ground` does the
  same for the background and foreground. The game turns it on only while it draws the world (`upscale::set_world`), so the HUD,
  menus and fonts keep the exe's pixels.
- **Exactness:** at zoom 1 (or with `--no-enhance` / `OPENSOW_ENHANCE=0`) output is byte-identical to before; the exe-exact RGB565
  blends are unchanged (they run on the enhanced pixels). `--gpu-check` still reports 0 differing pixels.
- Try it: `opensow-game --play M01 --zoom 2 --size 1920x1080 --screenshot out/x.png` vs the same with `--no-enhance`;
  `cargo run --release -p opensow-viewer --example upscale_preview -- DIR` compares the filters side by side.
- Options page 2 has the on/off switch (docs/options.md).
- Ideas not done: deblocking of the JPEG ground, an ML backend behind the same `enhanced()` hook.
