# First-run setup wizard

Spec: docs/specs/2026-11-03-first-run-setup-wizard.md
Date: 2026-11-03
Status: implemented
Area: vcmp-app (startup, `ui/setup.slint`, Settings, the update check)

## What was done

- `src/setup.rs` decides at startup whether the wizard opens.
  - It writes `ui.setup_done` when the wizard shows, and silently for a library
    that has music folders.
  - It honours `VCMP_SETUP=skip`, which writes nothing.
  - Only `main.rs` calls it, and only once the library view is connected.
- `ui/setup.slint` holds the `Setup` global and `SetupWizard`, a `Modal` with five
  steps: Welcome, Music folders, Look, Online services and Done.
  - It has Back, Next or Close, and Skip setup.
  - Skip and Escape ask through `Confirmation`. On Done, Close and Escape close it
    without asking.
  - The focus starts on Next, moves to each step's first control, and goes round
    the dialog.
  - The card keeps one height (`Theme.dialog-setup-height`, with a new
    `Modal.tall-limit`), and steps scroll inside it.
- The rows the wizard shares with Settings are now components:
  - `DiscordRow`, `ThemeRow`, `ZoomRow` and `MotionRow` in `ui/setting-rows.slint`
  - `UpdateCheckRow` and `AgentsSwitchRow`
  - `LibrarySection` with its heading optional
  Settings uses the same components and looks the same.
- "Run setup again..." is the first row of Settings, and the focus returns to it
  when the wizard closes.
- `updates::connect_held` holds the update check while the wizard is open at
  startup. It runs when the wizard closes, if the switch is still on.
- Tests are in `tests/setup.rs` (9). They fail when the confirmation or the hold is
  removed. Three Settings tests in `ui_behaviour.rs`, `lastfm.rs` and
  `preferences.rs` got taller windows or one more Tab, because the new row moves
  the page down.
- The screenshots example has `setup-*` shots of each step in both themes.
- Docs updated: `docs/ui.md` ("Setup wizard", the files table, "Update check"),
  `docs/library.md`, `docs/development.md`, `docs/manual-testing.md`, `README.md`
  ("The first start") and `CLAUDE.md` ("Run locally"). The README's "ShitHub" is
  now "GitHub".
- Checks: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D
  warnings` are clean, and `cargo test --workspace` passes 572 tests with none
  failing.

## How to test

**Automated coverage.** `cargo test -p vcmp-app --test setup` covers:
- the start rule
- the flag
- `VCMP_SETUP=skip`
- Next, Back and Close
- Skip and Escape with the confirmation
- the focus going round the dialog
- Run setup again with the current values
- the update check waiting for the wizard

They run headless through Slint's testing backend, the repository's UI test
pattern; there is no other E2E suite. `VCMP_SHOT_ONLY=setup cargo run -p vcmp-app
--example screenshots` renders every step in both themes into `target/screenshots/`.

**Setup.** An empty data directory: `VCMP_HOME=target/first-start cargo run -p
vcmp-app`. Delete `target/first-start` to see the first start again. For the Last.fm
and photo key rows, build with `VCMP_LASTFM_API_KEY`, `VCMP_LASTFM_SHARED_SECRET`
and `VCMP_FANARTTV_API_KEY` (docs/development.md, "Integration keys").

**Steps.**
1. Start VCMP as above. The "Set up VCMP" dialog covers the Albums page and shows
   "Step 1 of 5" and "Welcome to VCMP". The focus is on Next, and the player bar
   still works.
2. Press Enter. "Music folders" shows with Add folder... focused. Add a folder of
   music: the scan line and progress appear, and albums fill the page behind.
3. Next. On "Look", choose Dark: the window behind turns dark at once. Zoom in:
   the dialog grows, and the step scrolls if it no longer fits.
4. Next. "Online services" lists Show on Discord (only in builds with Discord),
   Scrobble to Last.fm (with the key), Check for updates (on), the fanart.tv key
   field (with the key) and Let AI agents use your library (off). Turn the agents
   switch on: a line points to Settings, AI agents.
5. Press Escape. "Close the setup?" appears. Choose Continue the setup: you stay on
   Online services.
6. Next. "All set" says how many tracks are in the library, or that the scan is
   still running. Close: the dialog goes without a question.
7. Quit and start again with the same `VCMP_HOME`. No setup appears.
8. Settings, Run setup again...: the wizard opens on Welcome. On Look, Dark is
   selected. Skip setup, then Close setup: the focus is back on Run setup again....

**Also worth trying.**
- Quit while the wizard is open, then start again: it doesn't come back.
- `VCMP_SETUP=skip` with an empty `VCMP_HOME`: no wizard. The same directory
  without the variable: the wizard appears.
- An existing library with music folders, started on this version: no wizard.
  `ui.setup_done` is `true` in its `settings` table.
- Turn Check for updates off in the wizard and close it: no request goes to
  github.com in that run.
- A 720 x 480 window at 200 % zoom: the buttons stay in view and each step scrolls.

## What is missing

Nothing - the spec is implemented in full.

## Known problems

- **Not tried by hand.** Nobody has run it interactively in the real app. The
  native folder dialog over the wizard, the Last.fm browser flow and the Discord
  row were checked only through tests and screenshots. The screenshot run has no
  Discord presence, so no shot shows that row.
- **Folder step focus.** The step focuses Add folder..., so on a rerun with folders
  listed, the folder rows above it are reached with Shift+Tab.
- **Empty card on tall windows.** The card stays 640 px tall on every step, so
  Welcome and Done leave much of it empty.
- **Focus after the first start.** When the wizard closes at the first start, the
  focus goes nowhere in particular. Space and the media keys still work through
  the window hook.
- **Failed library view.** If the database opens but the library view fails to
  load, the wizard doesn't open and the flag isn't written, so it appears on a
  later start that works.

## Recommended follow-ups

- Add the Home page and favourites to the Welcome text, and decide on a start-page
  step, when those features land.
- Add a screenshot of the Online services step with a fake Discord presence, so
  the Discord mention can be checked by eye.
- Size the card to its step's content with a minimum height, if the empty space on
  Welcome and Done turns out to bother people.

## Changelog

- Show a short setup on the first start: music folders, theme, zoom and motion, and
  what VCMP may send online, including showing what you play on Discord
- Add "Run setup again" at the top of Settings
- Wait with the update check on the first start until the setup has closed
