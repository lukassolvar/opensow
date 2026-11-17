//! The window: winit events are passed on to the `Game`, its frame is scaled into the window and presented (GPU or
//! softbuffer), and the mission pointer is set as the system cursor.

use super::*;

/// Where a `iw` x `ih` picture lands in a `ww` x `wh` window: the largest centred rectangle of the same aspect `(x, y, w, h)`.
pub(super) fn fit_rect(ww: usize, wh: usize, iw: usize, ih: usize) -> (usize, usize, usize, usize) {
    let s = (ww as f64 / iw as f64).min(wh as f64 / ih as f64);
    let (dw, dh) = (((iw as f64 * s).round() as usize).clamp(1, ww), ((ih as f64 * s).round() as usize).clamp(1, wh));
    ((ww - dw) / 2, (wh - dh) / 2, dw, dh)
}

/// Scales `src` (`iw` x `ih`) into `dst` (`ww` x `wh`) inside `rect` (nearest neighbour); the rest is black.
pub(super) fn blit_scaled(dst: &mut [u32], ww: usize, wh: usize, src: &[u32], iw: usize, ih: usize, rect: (usize, usize, usize, usize)) {
    let (ox, oy, dw, dh) = rect;
    if (dw, dh) == (iw, ih) && (ww, wh) == (iw, ih) {
        dst.copy_from_slice(src);
        return;
    }
    dst.fill(0);
    let xs: Vec<usize> = (0..dw).map(|x| (x * iw / dw).min(iw - 1)).collect();
    for y in 0..dh {
        let sy = (y * ih / dh).min(ih - 1);
        let (row, out) = (&src[sy * iw..(sy + 1) * iw], &mut dst[(oy + y) * ww + ox..(oy + y) * ww + ox + dw]);
        for (o, &sx) in out.iter_mut().zip(&xs) {
            *o = row[sx];
        }
    }
}

impl App {
    /// In a mission the pointer is a custom system cursor made from `cursor.ps6` (the same frame and hotspot the software
    /// pointer uses); elsewhere, or when the system refuses the image, the normal arrow (menus) / the frame's own drawing.
    pub(super) fn update_cursor(&mut self, el: &ActiveEventLoop, h: usize) {
        let Some(win) = self.window.clone() else { return };
        // `OPENSOW_SOFT_POINTER=1` keeps the old pointer drawn into the frame
        let want = self.game.custom_pointer() && std::env::var_os("OPENSOW_SOFT_POINTER").is_none();
        if let Some(p) = self.game.play.as_mut() {
            p.system_pointer = false;
        }
        if !want {
            if self.cursor_key.take().is_some() {
                win.set_cursor(Cursor::default());
            }
            // the software pointer (menus: none) is drawn into the frame, so the system's one is hidden behind it
            win.set_cursor_visible(!self.game.custom_pointer());
            return;
        }
        let ui = (h as f32 / 768.0).max(1.0);
        // macOS sizes a custom cursor in points, the other systems in pixels
        // the picture is stretched into the window: the cursor is as big as the game's pixels are on screen
        let (ww, wh) = self.window_size();
        let (iw, ih) = self.size();
        let shown = fit_rect(ww, wh, iw, ih).3 as f32 / ih as f32;
        let ui = ui * shown;
        let ui_img = if cfg!(target_os = "macos") { ui / self.game.scale.max(1.0) } else { ui };
        let Some(play) = self.game.play.as_ref() else { return };
        let Some((key, hot, w, h2, rgba)) = play.pointer_cursor(&mut self.game.assets, ui_img) else {
            win.set_cursor_visible(false);
            return;
        };
        if self.cursor_key != Some(key) {
            if let std::collections::hash_map::Entry::Vacant(e) = self.cursors.entry(key) {
                if let Ok(src) = CustomCursor::from_rgba(rgba, w as u16, h2 as u16, hot.0 as u16, hot.1 as u16) {
                    e.insert(el.create_custom_cursor(src));
                }
            }
            if let Some(c) = self.cursors.get(&key) {
                win.set_cursor(Cursor::Custom(c.clone()));
                self.cursor_key = Some(key);
            }
        }
        win.set_cursor_visible(self.cursor_key.is_some());
        if let Some(p) = self.game.play.as_mut() {
            p.system_pointer = self.cursor_key.is_some();
        }
    }

    /// The window's pixel size.
    pub(super) fn window_size(&self) -> (usize, usize) {
        self.window.as_ref().map_or((1280, 800), |w| (w.inner_size().width.max(1) as usize, w.inner_size().height.max(1) as usize))
    }

    /// The resolution the game renders at: the Options setting (any window size shows it scaled), or the window's own
    /// pixel size for NATIVE.
    pub(super) fn size(&self) -> (usize, usize) {
        if self.game.options.native() {
            let (w, h) = self.window_size();
            return (w.clamp(320, 7680), h.clamp(240, 4320));
        }
        let c = &self.game.options.cfg;
        (c.width.clamp(320, 7680) as usize, c.height.clamp(240, 4320) as usize)
    }

    /// A window point (pixels) -> the game's pixel; points on the bars stick to the nearest edge.
    pub(super) fn to_game(&self, x: f64, y: f64) -> (i32, i32) {
        let (ww, wh) = self.window_size();
        let (iw, ih) = self.size();
        let (ox, oy, dw, dh) = fit_rect(ww, wh, iw, ih);
        let gx = ((x - ox as f64) * iw as f64 / dw as f64).floor().clamp(0.0, iw as f64 - 1.0);
        let gy = ((y - oy as f64) * ih as f64 / dh as f64).floor().clamp(0.0, ih as f64 - 1.0);
        (gx as i32, gy as i32)
    }
}

/// The application icon at 64x64 RGBA for the window (made by build.rs from the launcher's `art/icon.png`; macOS
/// ignores window icons and shows the bundle's).
const WINDOW_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon64.rgba"));

/// The Wayland app id and X11 class `opensow`, the launcher's: the desktop shows the game under the launcher's
/// `opensow.desktop` entry and icon (opensow-launcher `desktop.rs`).
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly"))]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, "opensow", "opensow");
    winit::platform::x11::WindowAttributesExtX11::with_name(attrs, "opensow", "opensow")
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly")))]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let c = &self.game.options.cfg;
        self.window_res = (c.width, c.height);
        let icon = winit::window::Icon::from_rgba(WINDOW_ICON.to_vec(), 64, 64).ok();
        let mut attrs = with_app_id(Window::default_attributes().with_title("OpenSOW").with_window_icon(icon));
        if !self.game.options.native() {
            let (rw, rh) = self.size();
            attrs = attrs.with_inner_size(LogicalSize::new(rw as f64, rh as f64));
        }
        if self.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));
        if std::env::var_os("OPENSOW_SOFTWARE").is_none() {
            self.gpu = gpu::Gpu::new(window.clone());
        }
        if self.gpu.is_none() {
            let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
            self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        }
        self.game.scale = window.scale_factor() as f32;
        self.window = Some(window);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.game.quit {
            el.exit();
            return;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        if self.gpu.is_some() {
            // the vsync'd present paces the frames at the display's refresh
            el.set_control_flow(ControlFlow::Poll);
        } else {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(16)));
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let (w, h) = self.size();
        let res = (self.game.options.cfg.width, self.game.options.cfg.height);
        if self.window_res != res {
            // a new resolution from the options menu: the window takes that size (it can still be resized by hand;
            // fullscreen and NATIVE keep the window as it is)
            self.window_res = res;
            if let Some(win) = self.window.as_ref().filter(|w| w.fullscreen().is_none() && !self.game.options.native()) {
                let _ = win.request_inner_size(LogicalSize::new(w as f64, h as f64));
            }
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
                self.last = now;
                let t0 = Instant::now();
                self.game.update(dt, w, h);
                let t1 = Instant::now();
                self.update_cursor(el, h);
                let t2 = Instant::now();
                let (ww, wh) = self.window_size();
                let (mut t3, mut t4) = (t2, t2);
                self.frame.resize(w * h, 0);
                if let Some(gpu) = self.gpu.as_mut() {
                    // a mission frame is recorded and drawn by the GPU; anything that cannot be recorded is drawn on the CPU
                    let mut replayed = false;
                    if self.game.gpu_drawable() && self.replay {
                        opensow_viewer::record::begin(w, h);
                        self.game.draw(&mut self.frame, w, h);
                        if let Some(rec) = opensow_viewer::record::end().filter(|r| !r.tainted) {
                            t3 = Instant::now();
                            gpu.present_recording(&rec, ww, wh, fit_rect(ww, wh, w, h));
                            replayed = true;
                        }
                    }
                    if !replayed {
                        self.game.draw(&mut self.frame, w, h);
                        t3 = Instant::now();
                        gpu.present(&self.frame, w, h, ww, wh, fit_rect(ww, wh, w, h));
                    }
                    t4 = Instant::now();
                } else {
                    let Some(surface) = self.surface.as_mut() else { return };
                    if surface.resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap()).is_err() {
                        return;
                    }
                    self.game.draw(&mut self.frame, w, h);
                    if let Ok(mut buf) = surface.buffer_mut() {
                        blit_scaled(&mut buf, ww, wh, &self.frame, w, h, fit_rect(ww, wh, w, h));
                        t3 = Instant::now();
                        let _ = buf.present();
                        t4 = Instant::now();
                    }
                }
                if let Some(p) = self.perf.as_mut() {
                    let ms = |a: Instant, b: Instant| b.duration_since(a).as_secs_f64() * 1000.0;
                    for (k, v) in [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, t4)].into_iter().enumerate() {
                        p.ms[k] += v;
                    }
                    p.frames += 1;
                    let since = *p.since.get_or_insert(now);
                    if now.duration_since(since).as_secs_f64() >= 1.0 {
                        let f = p.frames.max(1) as f64;
                        eprintln!(
                            "PERF {w}x{h}: {} fps; per frame update {:.1} ms, cursor {:.1} ms, draw {:.1} ms, present {:.1} ms",
                            p.frames,
                            p.ms[0] / f,
                            p.ms[1] / f,
                            p.ms[2] / f,
                            p.ms[3] / f
                        );
                        *p = Perf { since: Some(now), ..Perf::default() };
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = self.to_game(position.x, position.y);
                self.game.mouse_moved(x, y, w, h)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && !matches!(button, MouseButton::Left | MouseButton::Right) && self.game.fullscreen_key(button) {
                    if let Some(win) = &self.window {
                        win.set_fullscreen(if win.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
                    }
                }
                let (x, y) = self.game.cursor;
                self.game.mouse_button(button, state == ElementState::Pressed, x, y, w, h);
            }
            WindowEvent::ModifiersChanged(mods) => {
                let st = mods.state();
                self.game.set_ctrl(st.control_key() || st.super_key());
            }
            WindowEvent::Focused(false) => {
                self.game.set_ctrl(false);
                if let Some(m) = self.game.map_editor.as_mut() {
                    m.release_keys();
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if self.game.wants_text() {
                    self.game.text_input(&text);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let Some(m) = self.game.skirmish_menu.as_mut() {
                    m.wheel(dy, w, h);
                } else if let Some(m) = self.game.map_editor.as_mut() {
                    m.wheel(dy, w, h);
                } else if self.game.screen.is_none() && self.game.results.is_none() {
                    if let Some(p) = self.game.play.as_mut().filter(|p| !p.paused) {
                        p.wheel(dy, w, h);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && matches!(event.physical_key, winit::keyboard::PhysicalKey::Code(c) if self.game.fullscreen_key(c)) =>
            {
                if let Some(win) = &self.window {
                    win.set_fullscreen(if win.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.game.numpad = event.location == winit::keyboard::KeyLocation::Numpad;
                use winit::keyboard::{KeyCode as K, PhysicalKey};
                self.game.digit = match event.physical_key {
                    PhysicalKey::Code(c) => [K::Digit0, K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9]
                        .iter()
                        .position(|&d| d == c)
                        .or_else(|| {
                            [K::Numpad0, K::Numpad1, K::Numpad2, K::Numpad3, K::Numpad4, K::Numpad5, K::Numpad6, K::Numpad7, K::Numpad8, K::Numpad9]
                                .iter()
                                .position(|&d| d == c)
                        })
                        .map(|d| d as u8),
                    _ => None,
                };
                self.game.code = match event.physical_key {
                    PhysicalKey::Code(c) => Some(c),
                    _ => None,
                };
                self.game.key(&event.logical_key, event.state == ElementState::Pressed, w, h);
                // typed characters for a text field (dead keys and keyboard layouts already composed by the window)
                if event.state == ElementState::Pressed && self.game.wants_text() {
                    if let Some(t) = event.text.as_ref().filter(|t| t.chars().all(|c| !c.is_control())) {
                        self.game.text_input(t);
                    }
                }
                if let Some(win) = &self.window {
                    win.set_ime_allowed(self.game.wants_text());
                }
                self.game.numpad = false;
                self.game.digit = None;
                self.game.code = None;
            }
            _ => {}
        }
    }
}
