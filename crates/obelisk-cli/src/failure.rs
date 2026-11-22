//! How a command fails: one stable code a program branches on, a sentence
//! for whoever reads the log, and Obelisk's own reason when the engine gave
//! one. docs/CLI.md lists every code.

/// Every code a command fails with. A test checks docs/CLI.md names each.
#[cfg(test)]
pub const CODES: &[&str] = &[
    USAGE,
    INTERNAL,
    LIBRARY_UNAVAILABLE,
    HELPERS_MISSING,
    KEYCHAIN_UNAVAILABLE,
    NOT_SIGNED_IN,
    SIGN_IN_EXPIRED,
    INPUT_MISSING,
    CREDENTIALS_REJECTED,
    GUARD_REJECTED,
    GUARD_UNSUPPORTED,
    LOGIN_EXPIRED,
    RATE_LIMITED,
    LOGIN_FAILED,
    NETWORK,
    STEAM_FAILED,
    NOT_OWNED,
    NO_WINDOWS_BUILD,
    NOT_INSTALLED,
    BUSY,
    ALREADY_RUNNING,
    ANOTHER_GAME_RUNNING,
    GAME_RUNNING,
    NOT_RUNNING,
    SESSION_RECOVERY_FAILED,
    CANCELLED,
    INSTALL_FAILED,
    LAUNCH_FAILED,
    SESSION_FAILED,
    ROSETTA_REQUIRED,
    CLOUD_CONFLICT,
    CLOUD_SYNC_FAILED,
    VOILE_FAILED,
    STEAM_COMPONENTS_FAILED,
    QUIT_FAILED,
    STILL_RUNNING,
    SETTINGS_FAILED,
];

pub const USAGE: &str = "usage";
pub const INTERNAL: &str = "internal";
pub const LIBRARY_UNAVAILABLE: &str = "library_unavailable";
pub const HELPERS_MISSING: &str = "helpers_missing";
pub const KEYCHAIN_UNAVAILABLE: &str = "keychain_unavailable";
pub const NOT_SIGNED_IN: &str = "not_signed_in";
pub const SIGN_IN_EXPIRED: &str = "sign_in_expired";
pub const INPUT_MISSING: &str = "input_missing";
pub const CREDENTIALS_REJECTED: &str = "credentials_rejected";
pub const GUARD_REJECTED: &str = "guard_rejected";
pub const GUARD_UNSUPPORTED: &str = "guard_unsupported";
pub const LOGIN_EXPIRED: &str = "login_expired";
pub const RATE_LIMITED: &str = "rate_limited";
pub const LOGIN_FAILED: &str = "login_failed";
pub const NETWORK: &str = "network";
pub const STEAM_FAILED: &str = "steam_failed";
pub const NOT_OWNED: &str = "not_owned";
pub const NO_WINDOWS_BUILD: &str = "no_windows_build";
pub const NOT_INSTALLED: &str = "not_installed";
pub const BUSY: &str = "busy";
pub const ALREADY_RUNNING: &str = "already_running";
pub const ANOTHER_GAME_RUNNING: &str = "another_game_running";
pub const GAME_RUNNING: &str = "game_running";
pub const NOT_RUNNING: &str = "not_running";
pub const SESSION_RECOVERY_FAILED: &str = "session_recovery_failed";
pub const CANCELLED: &str = "cancelled";
pub const INSTALL_FAILED: &str = "install_failed";
pub const LAUNCH_FAILED: &str = "launch_failed";
pub const SESSION_FAILED: &str = "session_failed";
pub const ROSETTA_REQUIRED: &str = "rosetta_required";
pub const CLOUD_CONFLICT: &str = "cloud_conflict";
pub const CLOUD_SYNC_FAILED: &str = "cloud_sync_failed";
pub const VOILE_FAILED: &str = "voile_failed";
pub const STEAM_COMPONENTS_FAILED: &str = "steam_components_failed";
pub const QUIT_FAILED: &str = "quit_failed";
pub const STILL_RUNNING: &str = "still_running";
pub const SETTINGS_FAILED: &str = "settings_failed";

/// Why a command failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    /// Obelisk's own reason, a localization key of its app such as
    /// `backend.steam.runtime.startFailed`, when the engine gave one.
    pub key: Option<String>,
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            key: None,
        }
    }

    /// A failure the engine reported as `reason`: the code that reason
    /// always means, else `otherwise`, the code of what was being done.
    pub fn engine(otherwise: &'static str, reason: impl AsRef<str>) -> Self {
        let (key, told) = read(reason.as_ref());
        let code = known(&key).unwrap_or(otherwise);
        Self {
            code,
            message: format!("{}: {told}", sentence(code)),
            key: Some(key),
        }
    }

    /// The process's exit status: 2 for a command line it could not read,
    /// else 1.
    pub fn status(&self) -> i32 {
        if self.code == USAGE { 2 } else { 1 }
    }
}

/// An engine reason's key, and the reason as a log tells it. The engine
/// gives a localization key, or a JSON object of a `key` and the `args` its
/// sentence takes, each of which may be such a reason too.
fn read(reason: &str) -> (String, String) {
    #[derive(serde::Deserialize)]
    struct Message {
        key: String,
        #[serde(default)]
        args: Vec<String>,
    }
    match serde_json::from_str::<Message>(reason) {
        Ok(message) if message.args.is_empty() => (message.key.clone(), message.key),
        Ok(message) => {
            let args: Vec<_> = message.args.iter().map(|arg| read(arg).1).collect();
            let told = format!("{} ({})", message.key, args.join(", "));
            (message.key, told)
        }
        Err(_) => (reason.to_owned(), reason.to_owned()),
    }
}

/// The code an engine reason's key means whatever was being done.
fn known(key: &str) -> Option<&'static str> {
    Some(match key {
        "backend.steam.friends.authenticationRequired" => SIGN_IN_EXPIRED,
        "backend.steam.auth.credentialsRejected" | "backend.steam.auth.invalidCredentialsInput" => {
            CREDENTIALS_REJECTED
        }
        "backend.steam.auth.guardRejected" => GUARD_REJECTED,
        "backend.steam.auth.unsupportedConfirmation" => GUARD_UNSUPPORTED,
        "backend.steam.auth.expired" => LOGIN_EXPIRED,
        "backend.steam.auth.rateLimited" => RATE_LIMITED,
        "backend.steam.auth.network"
        | "backend.steam.cm.disconnected"
        | "backend.steam.cm.timeout"
        | "backend.steam.cm.directoryUnavailable" => NETWORK,
        "backend.steam.client.gameNotOwned" | "backend.steam.runtime.ownershipFailed" => NOT_OWNED,
        "backend.steam.client.noCompatibleDepot" => NO_WINDOWS_BUILD,
        "backend.steam.runtime.sessionBusy"
        | "backend.steam.client.downloadInProgress"
        | "backend.steam.client.installationLocked" => BUSY,
        "backend.steam.runtime.cancelled"
        | "backend.steam.content.cancelled"
        | "backend.steam.components.cancelled"
        | "backend.voile.cancelled"
        | "cancelled" => CANCELLED,
        "backend.runtime.rosettaUnavailable" => ROSETTA_REQUIRED,
        "backend.voile.downloadFailed"
        | "backend.voile.damaged"
        | "backend.voile.storageFailed" => VOILE_FAILED,
        "backend.steam.components.downloadFailed"
        | "backend.steam.components.damaged"
        | "backend.steam.components.storageFailed"
        | "backend.steam.components.unavailable" => STEAM_COMPONENTS_FAILED,
        _ => return None,
    })
}

/// What a code says to whoever reads the log.
fn sentence(code: &str) -> &'static str {
    match code {
        SIGN_IN_EXPIRED => "Steam asks the account to sign in again",
        CREDENTIALS_REJECTED => "Steam refused the account name or password",
        GUARD_REJECTED => "Steam refused the Steam Guard code",
        GUARD_UNSUPPORTED => "Steam asks for a confirmation this program cannot give",
        LOGIN_EXPIRED => "the sign-in expired before it was approved",
        RATE_LIMITED => "Steam asks to wait before another sign-in",
        NETWORK => "Steam could not be reached",
        NOT_OWNED => "the account does not own the game",
        NO_WINDOWS_BUILD => "the game has no Windows build the account can install",
        BUSY => "another operation holds the game",
        CANCELLED => "it was cancelled",
        ROSETTA_REQUIRED => "the game needs Rosetta, which is not installed",
        VOILE_FAILED => "the Voile runtime could not be installed",
        STEAM_COMPONENTS_FAILED => "Valve's Steam components could not be installed",
        INSTALL_FAILED => "the installation failed",
        LAUNCH_FAILED => "the launch failed",
        SESSION_FAILED => "the game's session failed",
        LOGIN_FAILED => "the sign-in failed",
        QUIT_FAILED => "the game could not be asked to quit",
        _ => "Steam failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_engine_reason_keeps_its_own_code_whatever_was_being_done() {
        let expired = Failure::engine(
            INSTALL_FAILED,
            "backend.steam.friends.authenticationRequired",
        );
        assert_eq!(expired.code, SIGN_IN_EXPIRED);
        assert_eq!(
            expired.key.as_deref(),
            Some("backend.steam.friends.authenticationRequired")
        );
        let rosetta = Failure::engine(LAUNCH_FAILED, "backend.runtime.rosettaUnavailable");
        assert_eq!(rosetta.code, ROSETTA_REQUIRED);
        let other = Failure::engine(LAUNCH_FAILED, "backend.steam.runtime.startFailed");
        assert_eq!(other.code, LAUNCH_FAILED);
        assert_eq!(
            other.message,
            "the launch failed: backend.steam.runtime.startFailed"
        );
    }

    #[test]
    fn a_reason_with_arguments_gives_its_key_and_tells_them() {
        let nested = r#"{"key":"backend.steam.client.depot","args":["42"]}"#;
        let reason = serde_json::json!({
            "key": "backend.steam.auth.http", "args": ["503", nested]
        })
        .to_string();
        let failure = Failure::engine(LOGIN_FAILED, &reason);
        assert_eq!(failure.code, LOGIN_FAILED);
        assert_eq!(failure.key.as_deref(), Some("backend.steam.auth.http"));
        assert_eq!(
            failure.message,
            "the sign-in failed: backend.steam.auth.http (503, backend.steam.client.depot (42))"
        );
    }

    #[test]
    fn only_an_unreadable_command_line_exits_with_2() {
        assert_eq!(Failure::new(USAGE, "unknown command").status(), 2);
        assert_eq!(Failure::new(NOT_SIGNED_IN, "no account").status(), 1);
    }

    #[test]
    fn every_code_is_snake_case_listed_once_and_documented() {
        let manual = include_str!("../../../docs/CLI.md");
        for (index, code) in CODES.iter().enumerate() {
            assert!(
                code.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{code}"
            );
            assert!(!CODES[..index].contains(code), "{code} is listed twice");
            assert!(
                manual.contains(&format!("| `{code}` |")),
                "docs/CLI.md does not list the error code {code}"
            );
        }
    }
}
