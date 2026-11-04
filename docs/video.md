# Videos (`.sgv`)

`Data/Levels/*.sgv` (plus localized `mI`/`mO` in `Data/DiffData/<lang>/Levels`) are briefing and intro
movies. The exe has **no decoder of its own**: the player object at `0x6b409c` creates a DirectShow
multimedia stream (`CoCreateInstance` of `IAMMultiMediaStream`, IID `bebe595c-9a6f-11d0-8fde-00c04fd9189d`),
adds a video stream bound to the game's DirectDraw surface and an audio stream, opens the path
(`0x457300` picks the path string by the language index, `0x4573e0` opens it, `0x457220` positions it
centred in a 640x480 rectangle, `0x4576c0` releases it). Playback and decoding are done by the OS codecs;
a port needs its own MPEG-1 and Indeo/MS-Video-1 decoders or an external one.

Paths are table entries `data\levels\m01.sgv … m22.sgv` (0x48ec93…), `mI.sgv` (intro, 0x48e938, used at
0x434dc0), `mO.sgv` (outro, 0x48e958, used at 0x43528f), `mA`/`mB` at 0x48f2d4/0x48f2e8.

| file | container | video | audio |
|---|---|---|---|
| M01–M22 | MPEG-1 system stream | 320x208, 25 fps, 3.2 Mbit/s, ~13-16 s | layer 2, 192 kbit/s, 44.1 kHz stereo |
| mO | MPEG-1 system stream | 320x240, 25 fps, 1.8 Mbit/s, 100 s | layer 2, 192 kbit/s stereo |
| mI | MPEG-1 system stream | 640x480, 29.97 fps, variable rate, 186 s | layer 2, 160 kbit/s stereo |
| mA | AVI | 640x480, 12.5 fps, 104 frames, Indeo 5 (`IV50`) | PCM 22.05 kHz mono 16 bit |
| mB | AVI | 128x112, 12.5 fps, 100 frames, MS Video 1 (`CRAM`, 8 bit) | none |

MPEG files: pack header (`00 00 01 BA`, MPEG-1 form `0010…`), system header, PES packets `E0` (video), `C0`
(audio), `BE` (padding). `mI` is padded with zero bytes to 2048-byte sectors before some packs.
GOPs are `I P P B B` style (no D pictures). The pack clock (SCR, 90 kHz) of the last pack agrees with
picture count / frame rate within a fraction of a second.

Code: `video.rs` (`parse` walks either container: packet/picture counts, size, rate, audio header, AVI
streams); `opensow sgv <file>`; `opensow check` parses every `.sgv`. Test: `intro_videos_are_plain_media_files`.

## MPEG-1 decoder (`mpeg1.rs`)

`video::video_stream` concatenates the video packet payloads (the elementary stream) and
`mpeg1::decode` returns the frames in display order (`Frame`: padded 4:2:0 planes, `to_yuv420`, `to_rgb`).
I, P and B pictures with half-pel motion compensation, custom quantiser matrices; no D pictures, no
error concealment (a bad stream is an `Err`). `opensow sgvframe <file> <n> out.png` writes one frame,
`opensow sgvyuv <file> out.yuv` all of them. Speed: all 22 mission videos plus mO and mI (about 17000
frames) take 11 s in release.

Verification: no MPEG-1 decoder ships on a stock Mac, so the reference was PyAV (FFmpeg) in a scratch venv,
never in the repo. Every frame of M01, M22, mO and mI (5578 frames, 640x480, B pictures) differs from
FFmpeg by at most 4 levels (mean 0.01-0.03): only the IDCT rounding differs (ours is floating point, FFmpeg's
integer). Frame counts match for all 24 files. The test `mpeg1_decoder_matches_an_independent_decoder`
pins mean luma and a pixel of three M01 frames.

The VLC tables were not typed from memory but **recovered from the reference decoder with probe streams**:
a hand-built one-macroblock picture whose block holds a candidate bit string, decoded by FFmpeg and
inverted with a forward DCT to read back (run, level). That gave all 111 coefficient codes of table B.14
(identical to FFmpeg's run/level arrays, Kraft sum fine) and the 63 coded-block-pattern codes (the standard's
texts disagree with several of my recollections). The macroblock-address, macroblock-type, motion and DC-size
tables are the standard's and were confirmed by decoding P and B pictures against FFmpeg.

## MPEG-1 layer II audio (`mp2.rs`)

`video::audio_stream` joins the audio packets, `mp2::decode` returns interleaved 16-bit PCM (`Audio`:
rate, channels, `to_wav`). `opensow sgvwav <file> out.wav` writes one. All files are 44.1 kHz stereo:
the 192 kbit/s movies (M01-M22, mO) use allocation table B.2b (30 subbands), the 160 kbit/s intro mI
table B.2a (27); the low-rate tables c/d are rejected (no file needs them). Frame = 12 groups of three
samples per subband, three scale-factor parts per 12 samples (`scfsi` selects which are shared), quantiser
classes of 3, 5 and 9 steps pack three samples into one code, value `(2k - (n-1)) / n` times the scale
factor `2^(1 - sf/3)`; polyphase synthesis with the 512-tap window. The window is the standard's
table (257 integers, taken from the reference decoder's data); the second half is the first half's mirror
negated except at multiples of 64 (the sign flips every 64 taps).

Verification: every sample of M01, M05, M13, M22, mO and mI differs from FFmpeg by at most 1 (rounding;
RMS 0.5), sample counts identical. The test `mp2_decoder_matches_an_independent_decoder` pins ten samples.

Gotcha found on the way: **the files contain several programs back to back** - a program end code
(`00 00 01 B9`) in the middle of M01 is followed by more packs (the audio runs on past the video), so the
walkers must skip the end code instead of stopping (stopping lost the last 0.3 s of audio).

## MS Video 1 (`msvideo1.rs`, mB)

`video::avi_chunks(data, 0)` gives the frame payloads, `msvideo1::Decoder` applies them (palette from the
`strf` chunk, `palette_from_strf`); `opensow sgvavi <file> <n> out.png` / `sgvaviyuv` export frames.
4x4 blocks, bottom block row first and bottom pixel row first inside a block; per block two bytes `a b`:
`b & 0xfc == 0x84` skips `((b-0x84)<<8) + a` blocks (this one included), `b < 0x80` is two colours with
mask `b:a` (bit set = first colour), `b >= 0x90` is eight colours (a pair per 2x2 quadrant) with the same
mask rule, anything else one colour `a`. Skipped blocks keep the previous picture, an empty chunk repeats it.
Verification: all 100 frames are bit-identical to FFmpeg's output; the test pins sums of three frames.

## Playing the intro (`opensow-game`)

`opensow-game` plays `mI.sgv` full screen before the menu (`intro.rs`): `mpeg1::Stream` (incremental
decoder) yields frames as the clock needs them, dropping frames if the machine is slow; the layer II track is
decoded up front and played through `rodio` (new dependency, `playback` feature only; no device = silent).
Any key or click skips it. `--no-intro` starts at the menu, `--lang SK|CZ|PL|HU|RU|CHINNESSE` picks the
language copy: the exe's path table (0x48e4bc, 0x48ff48...) maps language index 0..6 to `data\levels\` and
`data\diffdata\{SK,CZ,PL,HU,RU,CHINNESSE}\levels\`. Headless: `--screenshot out.png --intro-frame N`.
### Briefings and outro

The exe plays a mission's briefing right before the mission starts: the start-mission function (0x434300)
asks the current mission number (`0x420b70`), a 23-way jump table (0x4345e8) picks `data\levels\m01.sgv ...
m23.sgv` (m23 has no file), the movie is opened (`0x4573e0`) and run until it ends or is skipped, then the
level loads. The intro (`mI`, function 0x434cf0) and outro (`mO`, 0x435200) are two cases of the same
command dispatcher (0x42d261 / 0x42d26a); what posts the outro command was not traced. mO is clearly the
ending (the armed forces defeat the Beho-Sunns, the Galileo station, a ship 7 light years from Earth), so the
game plays it after winning the last campaign mission (`FINAL_LEVEL = "M22"`, an assumption).

In `opensow-game`: choosing a mission plays its briefing (`Game::begin`), then starts it; restarting from the
pause menu does not replay it; winning M22 plays `mO` before the closing message. Same skip keys, `--mute`
opens no audio device, `--no-intro` turns every movie off. Headless: `--movie M05 --intro-frame 120`.
The briefing movies have no language copies (only mI/mO do); the player falls back to `Data\Levels`.

## How the exe uses the two AVIs

`mA.sgv` (the green BRIEFING animation) is opened at (0, 0) by the briefing screen's code at 0x441d2d, with the music
track `Brief`/`Brief2` chosen by a coin flip at 0x441cf9; `mB.sgv` (128x112) at x = 0x20, y = 1 (the `0x20` is the x, not a flag) at 0x4413e6, 0x44148a,
0x441553 and 0x441c7f, around the same screen's state changes (docs/lands.md). Both go through the same DirectShow player
object as the MPEG files.

Indeo 5 (mA): decoded by `indeo5.rs`, bit-exact against FFmpeg for all 104 frames (all intra, docs/indeo5.md); 

In the game (`intro.rs::Anim`, `lands.rs`): "New game" plays `mA` full screen (with its PCM sound), then the campaign map appears
with `mB` looping at (32, 1) (the exe's open call); see docs/lands.md. The mission
briefings M01..M22 are plain full-screen movies played when a mission is accepted. `opensow sgvavi mA.sgv N out.png` writes an mA frame.
