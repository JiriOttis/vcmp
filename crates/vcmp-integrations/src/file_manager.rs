//! The system's file manager: opening a folder, and showing a file selected in its
//! folder. Explorer on Windows and the Finder on macOS select the file themselves; on
//! Linux the file manager is asked over D-Bus (`org.freedesktop.FileManager1`, which
//! Nautilus, Dolphin, Nemo, Caja and Thunar answer), and where none answers, the
//! file's folder opens instead.

use std::io;
use std::path::Path;
use std::process::{Child, Command};

/// Opens the folder in the system's file manager.
pub fn open_folder(folder: &Path) -> io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    reap(Command::new(program).arg(folder).spawn()?);
    Ok(())
}

/// Shows the file in the system's file manager, selected in its folder. Returns
/// before the file manager has answered; on Linux the folder opens when no file
/// manager takes the request.
pub fn reveal(file: &Path) -> io::Result<()> {
    platform::reveal(file)
}

/// Waits for the program on a thread of its own, only to reap it. Explorer reports
/// failure even when it opened the window, so its status says nothing.
fn reap(mut child: Child) {
    std::thread::spawn(move || child.wait());
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    pub fn reveal(file: &Path) -> io::Result<()> {
        // Explorer reads "/select," and the path as one argument, the path quoted
        // on its own; Rust's quoting would put the quotes around both.
        let child = Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn()?;
        super::reap(child);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::io;
    use std::path::Path;
    use std::process::Command;

    pub fn reveal(file: &Path) -> io::Result<()> {
        super::reap(Command::new("open").arg("-R").arg(file).spawn()?);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use zbus::blocking::{Proxy, connection};

    /// Long enough for D-Bus to start the file manager on the first request.
    const TIMEOUT: Duration = Duration::from_secs(5);

    pub fn reveal(file: &Path) -> io::Result<()> {
        let file = file.to_path_buf();
        // A D-Bus round trip, and maybe the file manager starting: not on the UI thread.
        std::thread::spawn(move || {
            if let Err(error) = show_items(&file) {
                log::info!("no file manager selected the file ({error}); opening its folder");
                let folder = file
                    .parent()
                    .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
                if let Err(error) = super::open_folder(&folder) {
                    log::warn!("could not open {}: {error}", folder.display());
                }
            }
        });
        Ok(())
    }

    fn show_items(file: &Path) -> Result<(), String> {
        let uri = url::Url::from_file_path(file)
            .map_err(|()| "the path is not absolute".to_owned())?
            .to_string();
        let connection = connection::Builder::session()
            .and_then(|builder| builder.method_timeout(TIMEOUT).build())
            .map_err(|error| error.to_string())?;
        let proxy = Proxy::new(
            &connection,
            "org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1",
        )
        .map_err(|error| error.to_string())?;
        proxy
            .call_method("ShowItems", &(vec![uri.as_str()], ""))
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use std::io;
    use std::path::Path;

    pub fn reveal(file: &Path) -> io::Result<()> {
        super::open_folder(file.parent().unwrap_or(file))
    }
}
