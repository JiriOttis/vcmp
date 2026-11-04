# First-run setup wizard

Status: agreed
Date: 2026-11-03
Area: vcmp-app (startup, a new dialog, Settings, preferences, the update check's
timing); vcmp-library only through its existing settings table; all three platforms

## Context of the change

A new user starts VCMP on an empty Albums page. It says the library is empty and
shows an "Add music folder..." button. Nothing introduces the app. The Discord
presence is on by default and nothing points it out. The v1 spec
([2026-10-17-vcmp-music-player.md](2026-10-17-vcmp-music-player.md)) kept Discord on
by default on the understanding that a setup wizard would later mention it. The
listening and library features spec
([2026-10-22-listening-and-library-features.md](2026-10-22-listening-and-library-features.md),
"First-run wizard") planned that wizard. It agreed on new libraries only with a
Settings button to run it again, on the steps Music folders, Look and Online
services after a welcome, and on a dialog over the window rather than full-window
pages. It meant to build the wizard last so it could introduce Home and favourites.
Issue #83 asks for it now, with high priority, and those features are not built yet.

What exists today on `main`:

- **Settings** (`ui/pages.slint`, `SettingsPage`) has every row the wizard needs:
  - music folders (`LibrarySection`), with Subfolders, Remove, Add folder and the
    scan progress
  - theme, zoom and reduce motion
  - "Show on Discord", only in builds with Discord, on by default
  - Last.fm (`LastfmRow`), only in builds with VCMP's key
  - the fanart.tv personal key, only in builds with VCMP's fanart.tv key
  - AI agents, off by default
  - "Check for updates", on by default
- **Dialogs** are the `Modal` frame (`ui/components.slint`). It has a scrim and
  keeps the focus ([ui.md](../ui.md), "A dialog keeps the focus"). The
  `Confirmation` global (`ui/confirmation.slint`) asks "are you sure?" questions
  and can show over another dialog.
- **Preferences** are `ui.*` keys in the library's settings table
  (`src/preferences.rs`). Without a database VCMP still runs, and settings last for
  the session.
- **The update check** (`src/updates.rs`) asks github.com once as VCMP starts, if
  its switch is on.
- **Tests and examples** build the window with `create_window()` and connect the
  services they need. Only `main.rs` wires the real startup.

## User request

The user asked to brainstorm and implement the feature in issue #83, after updating
`main` to the latest version. The issue asks for a first-run setup wizard. A new
user starts on an empty Albums page with no introduction, and the Discord presence
is on by default without being mentioned anywhere.

The wizard is shown when the database has no music folders and no `ui.setup_done`
setting. Finishing or skipping sets the setting. An existing library sets it
silently on its first start after the update. The wizard is a `Modal` over the
window with Welcome, Music folders, Look, Online services and Done. Online services
shows only what the build has (Last.fm, Discord, the update check and so on) and
mentions the Discord presence. Current defaults are kept. "Run setup again" in
Settings opens it with the current values. Tests and screenshots with a fresh
`VCMP_HOME` need a way to skip it.

The dialog keeps the focus like the other modals. The issue asks for:

- tests for when it shows, skip and finish, the silent setting for existing
  libraries, and running it again
- updates to `docs/ui.md`, `docs/manual-testing.md` and `README.md`'s first-start
  section
- screenshots of each step in both themes
- every new string through `@tr()`

Showing it to existing users and a start-page step are out of scope.

During the session the user added:

- A flag marks whether the user has seen the wizard.
- The wizard can be skipped.
- Once seen, it appears only when the user opens it with the button in Settings.
- Skip and Escape show a popup saying that this closes the wizard, and ask whether
  to continue.

The issue sets the flag on finishing or skipping. The later session answer says
"once user sees it once", and the user confirmed that the flag is set as soon as
the wizard shows.

## Decisions

### Which libraries count as "existing", so the wizard stays hidden and `ui.setup_done` is set silently on their first start after the update?

| Option | What it means |
|---|---|
| Has a music folder (Recommended) | Exactly the issue's rule; someone who opened VCMP before but never added a folder still gets the wizard once. |
| Folder or any setting | Also hides it from anyone who already changed a setting (theme, zoom...); fewer surprises for testers, but a few real new users miss it. |

**Answer: "I think there should be some kind of flag that marks if user has seen the wizard or not. We should allow to skip the wizard, and once user sees it once, it should only appear when they manually force that using a button in settings"**

### What happens when the library database can't be opened (VCMP still runs, settings last for the session only)?

| Option | What it means |
|---|---|
| No wizard (Recommended) | Folders can't be saved anyway and `ui.setup_done` can't be stored, so it would come back on every start. |
| Show it every start | Still introduces the app, but repeats each launch until the database works and its folder step can't do anything. |

**Answer: No wizard**

### When is the "seen" flag set?

Asked to clarify the first answer.

| Option | What it means |
|---|---|
| As soon as it shows (Recommended) | Matches "once user sees it": closing VCMP mid-wizard or a crash also counts as seen; the Settings button is the way back. |
| On finish or skip only | Quitting halfway brings it back next start; a user who closes the window to get rid of it sees it again. |

**Answer: As soon as it shows**

### Existing users updating to this version have no flag yet. Do they see the wizard once?

| Option | What it means |
|---|---|
| No, set it silently (Recommended) | A database with a music folder gets the flag without showing anything, as the earlier spec agreed; they can open it from Settings. |
| Yes, everyone once | Every user sees it once after updating; introduces things they may have missed, but walks them through choices already made. |

**Answer: No, set it silently**

### What does the Welcome step say? (Home and favourites aren't built yet.)

| Option | What it means |
|---|---|
| Short intro (Recommended) | Two or three sentences on what VCMP is and what the next steps set up; little to keep current as features land. |
| Feature tour | A short list of what VCMP does (browse, labels, tag editing, covers, agents...); more informative, but goes stale with each feature. |

**Answer: Short intro**

### What does the Music folders step show?

| Option | What it means |
|---|---|
| Settings' folder section (Recommended) | The same list with Subfolders, Remove, Add folder and the scan progress; one component to keep, the scan runs behind the dialog. |
| Just an add button | One "Add music folder..." button and the chosen folder names; simpler, but no subfolder choice or removal until Settings. |

**Answer: Settings' folder section**

### What does the Look step hold besides theme and reduce motion?

| Option | What it means |
|---|---|
| Theme and motion only (Recommended) | As the earlier spec agreed; both apply live behind the dialog. |
| Add zoom too | Helps on large or high-DPI monitors at first start; one more control in a short step. |

**Answer: Add zoom too**

### Which rows does the Online services step have? Each shows only when the build has it, with Settings' wording on what it sends where.

Several could be chosen.

| Option | What it means |
|---|---|
| Discord presence (Recommended) | Toggle, on by default; the reason the wizard exists, so it is always in when the build has it. |
| Update check (Recommended) | Toggle, on by default; tells the user GitHub sees their IP address at each start. |
| Last.fm (Recommended) | Settings' Connect button that opens the browser; the step shows the waiting and connected states as Settings does. |
| AI agents and fanart.tv key | Agents' switch (off) and the optional photo key; more complete, but agents are local, not online, and the key is for few users. |

**Answer: Discord presence, Update check, Last.fm, AI agents and fanart.tv key** (all four)

### On the first start the update check runs before the user can turn it off in the wizard. What should happen?

| Option | What it means |
|---|---|
| Wait for the wizard (Recommended) | While the wizard is open the check doesn't run; it runs when the wizard closes, if still on. Nothing goes online before the user saw the choice. |
| Run at start as now | Simplest; one request to github.com before the user could say no. |

**Answer: Wait for the wizard**

### How much of AI agents goes into the wizard? (Settings has the switch, token, setup commands, playback switch, port, change history and backups.)

| Option | What it means |
|---|---|
| Switch only (Recommended) | "Let AI agents use your library" with its description, off by default; when on, a line says the token and setup commands are in Settings. |
| Switch and setup | Also the token and the Claude Code / Claude Desktop commands to copy; complete in place, but makes the step long and technical. |

**Answer: Switch only**

### What does the Done step show?

| Option | What it means |
|---|---|
| Summary and pointers (Recommended) | A line on the running scan (from the library's scan state) and that everything is in Settings, incl. Run setup again; then Close. |
| No Done step | The last step's button reads Finish and closes the dialog; one step fewer, but no place to say where things live. |

**Answer: Summary and pointers**

### How is the wizard left early? (Choices apply live, like Settings, so leaving early keeps what was already changed.)

| Option | What it means |
|---|---|
| Skip button and Escape (Recommended) | "Skip setup" on every step and Escape close it; a click beside the dialog does nothing, so a stray click doesn't end it. |
| Also a click beside it | Like the tag editor, the scrim click closes it too; quicker to dismiss, easier to lose by accident. |

**Answer: "Skip button and escape, but add a popup saying that this action will close the wizard and if user wants to continue"**

### Where does "Run setup again" sit in Settings?

| Option | What it means |
|---|---|
| Top of Settings (Recommended) | A row above Music folders; easy to find for anyone who skipped, but always takes the first line of the page. |
| Bottom of Settings | Its own row after Updates; out of the way, but at the end of a long page. |
| About page | Next to the version and credits; tidy Settings, but not where people look for settings. |

**Answer: Top of Settings**

### How do tests, examples and a fresh `VCMP_HOME` avoid the wizard?

| Option | What it means |
|---|---|
| Decided in main only (Recommended) | The show-or-not check is its own connect that only the real startup calls; tests and examples never see it unless they open it, and the screenshots example opens each step itself. No new switch. |
| Plus VCMP_SETUP env var | The same, and `VCMP_SETUP=skip` also hides it from `cargo run` with a fresh VCMP_HOME; handy for manual checks, one more variable to document. |

**Answer: Plus VCMP_SETUP env var**

### When the wizard was opened from Settings, does Skip / Escape ask the same confirmation?

| Option | What it means |
|---|---|
| Yes, same everywhere (Recommended) | One behaviour to learn and test; a rerun user closing it early sees the same question. |
| No, close at once | A rerun user chose to open it and already knows Settings; no extra click to leave. |

**Answer: Yes, same everywhere**

## High-level plan

**1. The startup rule.** The check that decides, once per start, whether the wizard
shows: the `ui.setup_done` flag, the silent flag for libraries that already have a
music folder, no wizard without a database, `VCMP_SETUP=skip`, and holding back the
update check while the wizard is open. Depends on nothing; testable without any UI.

**2. Shared setting rows.** The Settings rows the wizard repeats (music folders,
theme, zoom, reduce motion, Discord, update check, Last.fm, the fanart.tv key, the
AI agents switch) become components both places use, bound to the same state.
Settings looks and behaves as before. Depends on nothing; can go alongside 1.

**3. The wizard dialog.** The `Modal` with its five steps (Welcome, Music folders,
Look, Online services, Done), Back and Next, "Skip setup" with its confirmation,
Escape, and keyboard focus. Depends on 2 for its contents and on 1 to be opened at
startup.

**4. Run setup again.** The row at the top of Settings that opens the wizard on its
first step with the current values. Depends on 3.

**5. Tests, screenshots and documentation.** Tests for each rule below, a screenshot
of every step in both themes, and the documentation listed in the issue. Also
`VCMP_SETUP` in `docs/development.md` and next to `VCMP_HOME` in `CLAUDE.md`'s "Run
locally". Grows with each phase rather than coming at the very end.

## Architecture decisions

**The flag and when it is written.**
- `ui.setup_done` is a key in the library's settings table, next to the other `ui.*`
  preferences. No schema change.
- At startup, with the database open and no `VCMP_SETUP=skip`:

| State at start | Result |
|---|---|
| Flag present | No wizard |
| No flag, at least one music folder | Flag written silently, no wizard |
| No flag, no music folder | Flag written, then the wizard shows |

- The flag is written when the wizard shows, not when it closes: quitting or a crash
  halfway counts as seen (the owner's rule: "once user sees it once, it should only
  appear when they manually force that using a button in settings"). It beat writing
  it on finish, which would bring the wizard back to someone who closed the window to
  get rid of it.
- "Existing library" means "has a music folder". A user who removed every folder
  before updating gets the wizard once, like a new user.
- Without a database there is no wizard: nothing it sets could be kept, and it would
  return on every start.
- `VCMP_SETUP=skip` hides the wizard and writes nothing, so the same `VCMP_HOME`
  started without the variable still shows it. Other values are ignored.

**Where the decision lives.** The show-or-not check is a connect step of its own
that only the real startup runs. Tests and examples build their windows from the
individual services and never meet the wizard unless they open it; the screenshots
example opens each step itself. The environment variable is for people running the
real app on a fresh `VCMP_HOME`.

**One source of truth for every value.** Each wizard control binds to the same global
state and callbacks as its Settings row: changes apply and save at once, behind the
dialog, exactly as in Settings. So the wizard has no state of its own beyond its
open flag and current step, "Run setup again" shows the current values without
loading anything, and skipping keeps whatever was already changed. The descriptions
(what each service sends where) are the Settings wording, so the two cannot drift.

**The steps.**

| Step | Contents |
|---|---|
| Welcome | Two or three sentences: what VCMP is and what the next steps set up |
| Music folders | The Settings folder section: list, Subfolders, Remove, Add folder, scan progress; the scan runs behind the dialog and the pages fill as it goes |
| Look | Theme, zoom and reduce motion, applied live |
| Online services | Discord presence, update check, Last.fm connect, the fanart.tv personal key, the AI agents switch; each only in builds that have it |
| Done | A line on the scan (running, finished, or no folders yet), that everything is in Settings including Run setup again, and Close |

- AI agents shows only "Let AI agents use your library" (off by default). When it is
  on, a line says the token and setup commands are in Settings, under AI agents.
- Online services mentions the Discord presence, which is on by default, in its own
  row. That mention is why the wizard was asked for in the v1 spec.
- The update check and AI agents rows are in every build, so Online services always
  has rows.
- A step longer than the dialog (many folders, a small window, a large zoom)
  scrolls inside the card. The buttons stay in sight.

**Leaving the wizard.**
- "Skip setup" on every step before Done, and Escape anywhere before Done, ask
  through the existing confirmation dialog whether to close the wizard. Cancel goes
  back to the same step. Going ahead closes it, and the changes already made stay.
- On Done, Close and Escape close it without asking, because the wizard is finished.
- The same rules apply when it was opened from Settings.
- A click beside the dialog does nothing.

**The update check waits.** When the wizard opens at startup, the update check
(which today runs as VCMP starts) runs only once the wizard closes, and only if it
is still on. Nothing goes to github.com before the user has seen the choice. The
Discord presence needs no such hold: with no music folder nothing plays, so nothing
is shown on Discord before the user reaches its row. A rerun from Settings changes
nothing about either.

**Keyboard and focus.** The dialog keeps the focus like the other `Modal`s
(`docs/ui.md`, "A dialog keeps the focus"). The focus starts on Next and moves to the
next step's first control when the step changes. After closing it returns to the
page (first start) or to the "Run setup again" button (rerun).

**What tests enforce.**
- When the wizard shows: no flag and no folder, flag present, folder present
  (silent flag), no database, and `VCMP_SETUP=skip`.
- The flag is written when the wizard shows.
- Skip and Escape ask first, Cancel keeps the step, and going ahead closes and keeps
  the changes. Done closes without asking.
- A rerun opens on Welcome with the current values. The update check is held back
  until the wizard closes at startup.
- The existing invariant tests (every string through `@tr()`, no colour literals or
  durations outside the theme) cover the new file.

## Weaknesses and risks

| Risk | Likelihood | Cost | Mitigation |
|---|---|---|---|
| Settings rows laid out for the wide Settings column look cramped in the 720 px dialog, which is narrower still in a 720 px window, or at a large zoom | Medium | Wrapped or clipped rows | Each step scrolls inside the card; the smallest-window and zoom screenshots of every step |
| Zoom changes in the Look step resize the dialog under the pointer | Medium | Mildly confusing, buttons move | Keep the zoom controls where the card's top edge is stable, or accept and note it in `docs/ui.md` |
| Lifting Settings rows into shared components changes Settings by accident | Low | Regressed Settings page | Settings screenshots before and after; the existing Settings tests |
| A crash while the wizard is open on the first start loses it for good | Low | Never introduced | Run setup again at the top of Settings; the Done step can't help here, so the README first-start section mentions it |
| Last.fm connect opens the browser and waits; a user who skips while it waits may not notice it finishing | Low | Confusion | The same states as in Settings; the connection carries on in the background as it does there |
| The confirmation dialog over the wizard is a dialog over a dialog | Low | Focus lost on close | The same stacking already works for undo inside Changes by agents; a test that the focus returns to the wizard |
| Home and favourites are not built, so the wizard was built before the features it was meant to introduce | Certain | The Welcome text says nothing about them | The short intro avoids listing features; when Home lands, its change decides whether the wizard mentions it |
| Windows and macOS (native folder picker over a modal, Discord, Last.fm browser flow) aren't covered by headless tests | High | Platform-only bugs | Steps in `docs/manual-testing.md` |

Left open:
- Exact wording of Welcome, Done, the skip confirmation and the step titles. Settle
  during implementation and keep it in the antislop copywriting rules.
- Whether the dialog uses the normal dialog width or a wider one for the folder
  list. Settle against the screenshots.
- That the Discord presence shows nothing in the user's status while nothing
  plays, which the "no hold for Discord" decision assumes. Confirm it in
  `vcmp-integrations` (`discord`) during implementation. If connecting alone
  shows something, hold it back like the update check.

## Out of scope

- Showing the wizard to existing users; they can open it from Settings.
- A start-page step, and any mention of Home or favourites until those exist.
- The AI agents token, setup commands, playback switch and port inside the wizard;
  they stay in Settings.
- Undoing the wizard's changes on skip; every change applies live, as in Settings.
- Resuming at the step where a previous wizard was left.
- `VCMP_SETUP` values other than `skip`, and a command-line flag for the same.
- A full-window, installer-like wizard; the wizard is a dialog over the window.
- Translations of the new strings beyond `@tr()` (v1 ships in English).
