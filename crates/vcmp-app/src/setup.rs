//! The first-run setup wizard: whether it shows when VCMP starts, kept as
//! `ui.setup_done` in the library database's settings. The dialog is
//! `ui/setup.slint`; "Run setup again" in Settings opens it from there.

use slint::ComponentHandle;
use vcmp_library::Library;

use crate::{AppWindow, Setup};

/// Setting key: the wizard was shown once, or the library had music folders when
/// this version first started ("true"). Only Run setup again opens it after that.
pub const SETUP_DONE: &str = "ui.setup_done";

/// `VCMP_SETUP=skip`: no wizard on this start, and nothing written, so the same
/// `VCMP_HOME` started without it still shows the wizard.
pub fn skip_requested() -> bool {
    std::env::var_os("VCMP_SETUP").is_some_and(|value| value == "skip")
}

/// Whether the wizard shows on this start. Without the flag it is written now: when
/// the wizard shows, since a wizard closed by quitting counts as seen, and silently
/// for a library that already has music folders.
pub fn first_start(library: &Library, skip: bool) -> vcmp_library::Result<bool> {
    if skip || library.setting(SETUP_DONE)?.is_some() {
        return Ok(false);
    }
    let show = library.sources()?.is_empty();
    library.set_setting(SETUP_DONE, "true")?;
    Ok(show)
}

/// Opens the wizard if this is the first start. Returns whether it opened. A flag
/// that can't be read or written shows nothing, rather than the wizard on every start.
pub fn connect(window: &AppWindow, library: &Library, skip: bool) -> bool {
    let show = first_start(library, skip)
        .inspect_err(|error| log::warn!("could not read or save {SETUP_DONE}: {error}"))
        .unwrap_or(false);
    if show {
        window.global::<Setup>().invoke_start();
    }
    show
}
