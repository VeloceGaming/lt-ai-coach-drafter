LT AI COACH DRAFTER 1.0.1
=========================

WHAT IT DOES
Replaces the draft screen's "Delegate to Head Coach" button. A look-alike button
("Delegate ban/pick to LT Coach", in the game's language) sits over the game's
own; the click falls through to the game's button, which starts the game's own
delegation. While it runs, every ban and pick for your side is LT AI Coach's
recommendation instead of the game coach's.

REQUIRES
  - the LT AI Coach app, running, with Settings > Delegate to LT Coach > "Let LT Coach
    draft in game" switched on (OFF by default - the mod is optional);
  - the user-side context the Coach already needs (it advises only when it knows
    which side you are).
Button states:
  - Coach ready ............ look-alike button over the game's; the click starts the game's
                             own delegation, answered by LT Coach.
  - Coach running, switch off  the same-looking button, but it takes the click itself and
                             shows a warning explaining how to turn the switch on. The
                             game's own delegation is NOT started.
  - Coach not running, or switched on but unable to recommend (no data / user side
    unknown) ............... nothing is shown; the game's own button works as normal.

Disable or remove "LT Drafter Probe" first - it spawns its own button and registers its
own draft hook.

SAFETY
It answers only when sure. If the Coach is absent, slow, recommending for the
other side, or if the board / candidate pool on screen does not match what the game
is asking about, the mod answers nothing and the game's own Head Coach decides that
turn. It never touches the opponent's AI and never presses Cancel.

FINISH BAN/PICK NOW (game 0.6.3+)
The game's "Finish Ban/Pick Now" button, next to Cancel while delegation runs, completes
the draft without asking the mod: everything left is decided by the game's own Head Coach,
not LT Coach. While LT Coach drafts, a line under the buttons says so.

SWAP-STAGE DELEGATION (experimental, native)
  The swap-stage Coach button normally applies the GAME's own arrangement. With this on,
  it applies LT Coach's. To do that the mod redirects the two calls to the game's
  compute_coach_swap_order (a 10-byte patch of the running game, never the .exe on disk).
  It does so ONLY IF the game exe is exactly one of the builds it was reviewed for
  (game 0.6.2 and 0.6.3: SHA-256, PE timestamp, both call sites' bytes); on any other
  build, such as after a game update, it stays off and the log says why. Everything else
  in the mod is update-proof; this is the one part that is not.

  To use it:
   1. In the Coach: Settings > "Delegate to LT Coach": switch on "Let LT Coach draft in
      game" AND "Also let LT Coach arrange the swap stage (experimental)".
   2. RESTART THE GAME with the Coach already running. The patch is made only on the
      title screen; turning the switch on later takes effect next launch.
   3. At the swap stage the Coach button reads "Delegate swap to LT Coach" and applies
      LT Coach's arrangement (log: "swap: LIVE ... replaced [..] with [..]").
  CHECKING WITHOUT WRITING (e.g. a newly added game build): an empty file
  swap_dry_run.enabled  in %LOCALAPPDATA%\LTTools\lt_ai_coach_drafter\ makes it only log
  "swap: DRY RUN apply call: would replace [..] with [..]" (read when a draft starts).
  OFF SWITCH: an empty file  disable.native  in that folder (or turn the Coach switch off
  and restart the game). Remove "LT Swap Detour Probe" if installed: it patches the same
  two places.
  The mod answers only when the Coach's fingerprint of YOUR picks, in draft order, equals
  the one on screen; otherwise the game's own arrangement stays.

LIMITS
  - The game's draft timer: its auto-ban does not consult mods, so the mod stops
    waiting 3 s before the timer ends.

LOG
  %LOCALAPPDATA%\LTTools\lt_ai_coach_drafter\drafter.log   (rotates at 512 KB)
Look for lines starting "answered" / "declined": each says why.

PROTOCOL
Loopback UDP 127.0.0.1:32146, Coach -> mod only. See LT_AiCoach/src-tauri/src/drafter.rs.

BUILD / TEST
  cargo build --release     -> target\release\lt_ai_coach_drafter.dll
  cargo test                (fixtures in tests/fixtures, shared with the Coach app)
Install: copy the DLL and mod.mod_info to <game>\mods\lt_ai_coach_drafter\ and enable it.
