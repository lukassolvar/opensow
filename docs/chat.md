# Chat line and cheat codes (decoded)

**Enter** (`0x41612d`, scan flag `+0x80` of `0x415ba0`) sets `[0x5baa9a]` (chat active) and `[0x5bce8b]` (the window procedure forwards typed characters). It is not limited to network games. Characters then reach `0x426c90(char)`, which edits the line (an MFC `CString` array `0x70f01c`, element 1 is the typed text, element 0 is empty):

| Char | Effect |
|---|---|
| 9 Tab | the line sent last (`0x70f288`) replaces the text |
| 0x1b Esc | clear the text (`0x426d0d`) |
| 8 Backspace | remove the last character |
| >= 0x20 | append while the text is narrower than `0x18b` = 395 px (`0x4284bf`, measured before appending) |
| 13 Enter | copy the text to `0x70f288`, clear it; **single player only** (`[0x575720] == 0`): compare with the cheat phrases below |

On Enter the key handler (`0x416021`) closes the chat flags; in a network game the text is sent as `1` / `2` + text (blue / green, `0x4295d0`) and shown as a message line.

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

## OpenSOW

`Play::chat_*` (open / type / backspace / recall / cancel / send), `World::apply_cheat`, `World::say_chat`; the phrases, prefix match, amounts and codes are the exe's (`formats::cheats`), the width limit is `cheats::MAX_LINE_WIDTH`. While the line is open it takes every key press (no hotkeys, no dev panel). A line that is not a cheat becomes a chat line of the local player, drawn in the message queue in light blue and expiring like a message (480 ticks). A cheat is not echoed. Not traced: where the original draws the typed line (OpenSOW puts it above the message slots, white on a dimmed box) and whether a single-player line that is no cheat is shown at all; the network match is not playable yet, so nothing is sent.

**Differs from the exe:** only printable ASCII (0x20..0x7e) is appended (`Play::chat_type`); the exe takes every character from 0x20 on.

**Differs from the exe:** Esc drops the text and closes the line (`Play::chat_cancel`); the exe's character handler only clears the text (`0x426d0d`), whether its key handler also closes the line was not traced.

**Differs from the exe:** the protected value `[0x5baae8]` and the counters' XOR checksums are not modelled; a cheat changes the counter only.
