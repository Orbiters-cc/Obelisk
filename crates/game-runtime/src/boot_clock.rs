//! The clock since this Mac booted: the boot's own session UUID (`kern.bootsessionuuid`) and the seconds since that
//! boot, sleep counted (`CLOCK_MONOTONIC` on macOS). A wall clock set forward or back never moves it; a reboot starts
//! another. What a record says of when its game ended, for a program that must wait a while after an early end.
use serde::{Deserialize, Serialize};

/// A moment on the boot clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootInstant {
    /// `kern.bootsessionuuid`, upper case.
    pub boot: String,
    /// Whole seconds since that boot.
    pub seconds: u64,
}

/// This boot's session UUID, asked of `sysctl` once per process (it never changes until the next boot).
#[cfg(target_os = "macos")]
fn boot() -> Option<String> {
    static BOOT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.bootsessionuuid"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let boot = String::from_utf8(out.stdout).ok()?.trim().to_ascii_uppercase();
        (out.status.success() && boot.len() == 36).then_some(boot)
    })
    .clone()
}

/// Now on the boot clock; `None` where it cannot be read.
pub fn now() -> Option<BootInstant> {
    #[cfg(target_os = "macos")]
    {
        // CLOCK_MONOTONIC on macOS counts while the Mac sleeps.
        let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        Some(BootInstant { boot: boot()?, seconds: time.tv_sec.max(0) as u64 })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn the_boot_clock_names_this_boot_and_goes_on() {
        let a = super::now().expect("readable on a Mac");
        let b = super::now().unwrap();
        assert_eq!(a.boot, b.boot);
        assert!(b.seconds >= a.seconds && a.seconds > 0);
        assert_eq!(a.boot.len(), 36);
    }
}
