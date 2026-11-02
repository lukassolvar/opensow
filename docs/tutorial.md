# The tutorial script (mission M00), partly decoded

State machine at `0x432d6e..0x433b6f`, state variable `[0x70f2cc]` (0..0x1b; -1 outside the tutorial, which also switches the computer opponent off and uses the fast production speed, docs/ai.md). Dispatcher: `jmp [state*4 + 0x433be0]` (28 cases). Every step plays the voice file `sounds\Tutorial\NN.wav` (`0%d.wav` below 10) and shows the tutorial string; the strings 151-... of `langEN.txt` (`i` = information with a right-arrow button, `!` = a task) are the steps in order.

| step | kind | what advances it (decoded from the case code) |
|---|---|---|
| 0, 1 | info | right arrow (welcome, map scrolling) |
| 2 | task | UI flag `[0x6bed20] == 1` (the player scrolled with the right mouse button) |
| 3 | task | the four arrow-key flags `[0x6bee34/37/39/3c]` were pressed |
| 4 | info | arrow ("Lesson A: the Advancer") |
| 5 | task | creates the Advancer if missing (`0x43a420`), waits for it to be selected and moved |
| 6 | info | |
| 7 | task | builds the training light factory (`0x439e30` + slots `0x40d870`, creates the Advancer `0x43a420`), waits until the player owns it (capture) |
| 8, 9 | task | an Advancer exists / the player's headquarters... reach 2 active slots (upgrade) |
| 10 | task | repair task: spawns damaged buildings and units, waits until a building reaches its maximum hit points |
| 11 | info | "Lesson B: the Turrets" |
| 12 | task | builds a gold mine and enemy units; waits for 200 credits |
| 13, 14 | task/info | the player placed a turret (a turret object of the player exists and is not under construction `[+0x2c] == 0`) |
| 15, 16 | task | "Lesson C: the Air Forces": air base appears (kind 104), waits for the airstrike flag `[0x6beef0]` |
| 17, 18 | task | the air strike was used |
| 19, 20 | task | "Lesson D: the Tech-levels": medium factory; waits for the research lab (kind 106) to be captured |
| 21 | task | research reaches 500 (`[0x517b04] >= 0x1f4`) |
| 22 | task | the medium factory (kind 102) slot 0 reaches tech level 1 (`[b+0x17c] == 1`) |
| 23-26 | task/info | "Lesson E: the final task": spawns the enemy base; step 25 waits until the enemy headquarters is gone |
| 27 | end | "Excellent! You have successfully completed the tutorial." |

Only the transition conditions listed were read from the code; the exact positions where the script places buildings and units (the `0x43a420` / `0x439e30` argument lists in steps 5, 7, 10, 12, 15, 19 and 24) are stored in the case code and were not tabulated. The game does not run the tutorial script yet (M00 loads as a plain skirmish without the computer opponent).
