# Chat line and cheat codes (decoded)

**Enter** (`0x41612d`, scan flag `+0x80` of `0x415ba0`) sets `[0x5baa9a]` (chat active) and `[0x5bce8b]` (the window procedure forwards typed characters). It is not limited to network games. Characters then reach `0x426c90(char)`, which edits the line (an MFC `CString` array `0x70f01c`, element 1 is the typed text, element 0 is empty):

| Char | Effect |
|---|---|
| 9 Tab | the line sent last (`0x70f288`) replaces the text |
| 0x1b Esc | clear the text (`0x426d0d`) |
| 8 Backspace | remove the last character |
| >= 0x20 | append while the text is narrower than `0x18b` = 395 px (`0x4284bf`, measured before appending) |
| 13 Enter | copy the text to `0x70f288`, clear it; **single player only** (`[0x575720] == 0`): compare with the cheat phrases below |

On Enter the key handler (`0x416021`) closes the chat flags and, in single player too, inserts the line (the last element of `0x70f01c`, as `1` / `2` + text: blue / green by `[0x575730]`) at the front of the shown chat lines `0x70f29c` with a counter of 720 (`0x70f2b0`); only in a network game it is also sent (`0x4295d0`). The cheat phrases are therefore shown as chat lines as well.

## Cheats (`0x426d98..0x428460`)

The comparison is a chain of `text[i] == c` with a length test before each character: a phrase matches when the text **starts with it** (trailing text is ignored), case sensitive. Only the local player (`[0x575730]`) is affected. Every effect also sets the protected value `[0x5baae8] = 1` and re-signs it (`0x4062a0`, an XOR checksum) and the changed counter is re-signed too.

| Phrase | Effect |
|---|---|
| `give me money` | credits +1000 (`[0x517af8 + 4p]`, no cap) |
| `speed up my research` | research +200 (`[0x517b04 + 4p]`) |
| `give me fighter` / `bomber` / `meteor storm` / `carryall` / `tripler` | `0x453e80(player, code)` with 32 / 30 / 34 / 31 / 33: one force joins the stock and its "ready" message plays; sound 49 (`airicon-come`) is requested too but the install lacks it |
| `give me one minute` | `[0x5baac4]++` (countdown minutes) unless the end flag `[0x5baa99]` is set |
| `draw fps` | `[0x5baad5] = 1`: `0x4302e0` draws "FPS: n" (string `0x48e928`) at x = screen width / 2, y = height - 10, black then white 2 px right |

`formats::cheats` has the table; `tests/real_data.rs::cheat_phrases_match_the_exes_key_function` rebuilds the phrases, amounts and codes from the exe's bytes.

## Warmonger (`0x425da0`)

`State of War Warmonger.exe` has its own copy of the character function, with other phrases for the same effects (same amounts, codes and order) and one cheat of its own checked first; its single-player test is `[0x666b98] == 0`, the local player `[0x666ba8]` (0 in single player, `0x421ec1`). It has no Tab case: the line sent last is not kept.

| Phrase | Effect |
|---|---|
| `rahman is son of a bitch` | the typed line's text becomes "Yeah! Let's Finish him!" (string `0x4837d8`), which the key handler (`0x41598c`) then shows as the local player's chat line; credits +2222 (`[0x608f70 + 4p]`); `0x44e360` (the base game's `0x453e80`) with 33, 31, 34, 30, 32: one tripler, carryall, meteor storm, bomber and fighter with their "ready" messages. No protected value is set; the normal phrases are compared afterwards (`0x426267`) |
| `bring me money` / `speed up research` | as `give me money` / `speed up my research` |
| `bring me fighter` / `bomber` / `meteor storm` / `carryall` / `tripler` | as `give me ...` |
| `bring me one minute`, `draw fps` | as `give me one minute`, `draw fps` |

The base game's phrases do nothing there (verified by emulating `0x425da0` with unicorn, MFC calls and `0x44e360` hooked: every phrase, wrong case, a cut-off phrase, network play, player indices). `tests/real_data.rs::warmonger_cheat_phrases_match_its_exe` rebuilds the phrases, the reply, the amount and the codes from its bytes.

## OpenSOW

`Play::chat_*` (open / type / backspace / recall / cancel / send), `World::apply_cheat`, `World::say_chat`; the phrases, prefix match, amounts and codes are the exe's (`formats::cheats`; `cheats::parse` takes the campaign's table, `WARMONGER_PHRASES` while Warmonger is played), the width limit is `cheats::MAX_LINE_WIDTH`. While the line is open it takes every key press (no hotkeys, no dev panel). Every sent line, cheat or not, becomes a chat line of the local player, drawn in the message queue in light blue and expiring like a message (480 ticks); for Warmonger's own cheat it is the reply. Tab brings back what was typed last in both campaigns (in the Warmonger campaign not in the exe, which has no Tab case). Not traced: where the original draws the typed line (OpenSOW puts it above the message slots, white on a dimmed box); the network match is not playable yet, so nothing is sent.

**Differs from the exe:** chat lines live 480 ticks like messages; the exe's own counter (720, `0x70f2b0`) was not traced further.

**Differs from the exe:** only printable ASCII (0x20..0x7e) is appended (`Play::chat_type`); the exe takes every character from 0x20 on.

**Differs from the exe:** Esc drops the text and closes the line (`Play::chat_cancel`); the exe's character handler only clears the text (`0x426d0d`), whether its key handler also closes the line was not traced.

**Differs from the exe:** the protected value `[0x5baae8]` and the counters' XOR checksums are not modelled; a cheat changes the counter only.
