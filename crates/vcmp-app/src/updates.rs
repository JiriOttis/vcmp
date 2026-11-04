//! The update check's switch in Settings, saved in the library database's settings,
//! and its note in the sidebar and on the About page. The check itself is
//! `vcmp_integrations::updates`.

use std::cell::Cell;
use std::rc::Rc;

use slint::ComponentHandle;
use vcmp_integrations::updates::{self, Release};
use vcmp_library::Library;

use crate::{AppWindow, UpdateState};

/// Setting key: check for updates when VCMP starts ("true" or "false"; absent: true).
pub const SETTING_UPDATE_CHECK: &str = "integrations.update_check";

/// Asks for a newer release; given the window to show it in.
type Check = Box<dyn FnOnce(slint::Weak<AppWindow>)>;

/// A check that waits, while the setup wizard is open on the first start, until
/// [`HeldCheck::release`]: nothing goes to GitHub before the user has seen the switch.
pub struct HeldCheck {
    held: Rc<Cell<bool>>,
    run: Rc<dyn Fn(&AppWindow)>,
}

impl HeldCheck {
    /// Lets the check run: now if the switch is still on, or when it is turned on.
    pub fn release(&self, window: &AppWindow) {
        self.held.set(false);
        if window.global::<UpdateState>().get_check_enabled() {
            (self.run)(window);
        }
    }
}

/// Applies the saved switch and saves it when the user flips it. `check` runs once
/// at most: now if the switch is on, or later when the user turns it on. Without a
/// database the switch lasts for the session.
pub fn connect(
    window: &AppWindow,
    library: Option<Rc<Library>>,
    check: impl FnOnce(slint::Weak<AppWindow>) + 'static,
) {
    connect_held(window, library, false, check);
}

/// As [`connect`], but with `held` the check waits for the returned
/// [`HeldCheck::release`], even with the switch on.
pub fn connect_held(
    window: &AppWindow,
    library: Option<Rc<Library>>,
    held: bool,
    check: impl FnOnce(slint::Weak<AppWindow>) + 'static,
) -> HeldCheck {
    let saved = library.as_ref().and_then(|library| {
        library
            .setting(SETTING_UPDATE_CHECK)
            .inspect_err(|error| log::warn!("could not read the update check setting: {error}"))
            .ok()
            .flatten()
    });
    let enabled = saved.as_deref() != Some("false");
    let state = window.global::<UpdateState>();
    state.set_check_enabled(enabled);

    let pending: Rc<Cell<Option<Check>>> = Rc::new(Cell::new(Some(Box::new(check))));
    let held = Rc::new(Cell::new(held));
    let run: Rc<dyn Fn(&AppWindow)> = Rc::new({
        let held = held.clone();
        move |window: &AppWindow| {
            if held.get() {
                return;
            }
            if let Some(check) = pending.take() {
                check(window.as_weak());
            }
        }
    });
    if enabled {
        run(window);
    }
    let weak = window.as_weak();
    let switched = run.clone();
    state.on_set_check_enabled(move |on| {
        let Some(window) = weak.upgrade() else { return };
        window.global::<UpdateState>().set_check_enabled(on);
        if let Some(library) = &library
            && let Err(error) =
                library.set_setting(SETTING_UPDATE_CHECK, if on { "true" } else { "false" })
        {
            log::warn!("could not save the update check setting: {error}");
        }
        if on {
            switched(&window);
        }
    });
    HeldCheck { held, run }
}

/// Asks GitHub on a worker thread and shows a newer release in the window. The check
/// `main` passes to `connect`.
pub fn check_online(window: slint::Weak<AppWindow>) {
    updates::start(env!("CARGO_PKG_VERSION"), move |release| {
        let _ = window.upgrade_in_event_loop(move |window| show(&window, &release));
    });
}

/// Shows that `release` is out. The note stays hidden while the check is off.
pub fn show(window: &AppWindow, release: &Release) {
    let state = window.global::<UpdateState>();
    state.set_release_url(release.url.as_str().into());
    state.set_version(release.version.as_str().into());
}
