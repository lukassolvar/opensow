# Launcher

`cargo run --release -p opensow-launcher` (code: `crates/opensow-launcher`, settings: `formats::launcher`).

Two things are set, and saved at once to `launcher.cfg` in the per-user data folder (`~/Library/Application Support/OpenSOW`, `%APPDATA%\OpenSOW`, `~/.local/share/opensow`, or `$OPENSOW_SAVE_DIR`):

- **Language**: each with its flag drawn in code (`flag_color`; English = UK, Chinese = PRC, traditional = Taiwan): English, Slovak, Czech, Chinese (simplified / traditional) or "From game files" (the install's `Data\lang.dat`, the original's own mechanism). `GameDir::language()` is what every menu now asks; the movie folder index (`--lang`) follows it (both Chinese scripts share the `CHINNESSE` folder). Polish, Hungarian and Russian are not offered (English or damaged text, no art; docs/text.md); a saved one reads as "From game files", and the game treats it as English.
- **Game folders**: `State of War` (a folder with `Data`) and `State of War Warmonger` (`GameDir::is_warmonger_dir`). Set by Browse (system folder dialog: osascript / zenity / PowerShell), by dropping a folder on the window (a Warmonger install always lands in its own card), or Auto-detect (env var, Steam depot path, sibling Warmonger folder). The Play button needs a valid State of War folder. Each card shows Found / Not set / Not a valid folder.

One Play button starts `opensow-game` (it opens on the campaign used last; the two campaigns are switched on its world map, docs/warmonger.md) from the folder of the launcher binary. Lookup order in the game (`GameDir::locate`): `OPENSOW_GAME_DIR` / `OPENSOW_WARMONGER_DIR`, then `launcher.cfg`, then the default Steam path / sibling folder.

File format: `language=CZ`, `original=/path`, `warmonger=/path`, one per line, unknown lines ignored.

The page is a fixed 600x520 layout scaled to the window (checked headless at 640x480, 800x600, 1920x1080). Text uses a system font (Arial Unicode on macOS, for the Cyrillic and Chinese names; a font lacking a script shows the English name with its code).

## Graphics
Everything is drawn in code from scratch; no game asset and no downloaded or third-party image is used (and no image file is stored in the repo). The backdrop (`paint_background`) is a dark gradient with faint contour lines of a made-up height field plus a coarse grid, drawn in page coordinates so it fills any window. The emblem (`emblem_pixel`: rounded badge, crosshair ring, star) is shown in the header and set as the window icon (`emblem_rgba`, 64x64; Windows/Linux; winit cannot set the macOS Dock icon). `opensow-launcher --icon-png out.png --size 256` renders it for a look.
