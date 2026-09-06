pub mod inventory;
pub mod uninstall;
pub mod scan;
pub mod delete;
pub mod locks;
pub mod ai;
pub mod verify;
pub mod export;
pub mod open_location;
pub mod history;
pub mod trash;
pub mod scheduler;
pub mod registry_backup;

use std::process::Command;

/// Spawn a console program (schtasks, sc.exe, powershell, tasklist) with
/// CREATE_NO_WINDOW so no CMD window flashes while it runs. GUI apps
/// (vendor uninstallers, explorer, msiexec dialogs) are unaffected — they
/// own their windows.
#[cfg(windows)]
pub(crate) fn silent(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(program);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(not(windows))]
pub(crate) fn silent(program: &str) -> Command {
    Command::new(program)
}
