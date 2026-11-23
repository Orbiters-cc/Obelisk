//! How a command fails: one stable code a program branches on, a sentence
//! for whoever reads the log, and Obelisk's own reason when the engine gave
//! one. docs/CLI.md lists every code.

use game_runtime::voile::engine_patch::{Action, Incoherent};

/// Every code a command fails with. A test checks docs/CLI.md names each.
#[cfg(test)]
pub const CODES: &[&str] = &[
    USAGE,
    INTERNAL,
    LIBRARY_UNAVAILABLE,
    HELPERS_MISSING,
    KEYCHAIN_UNAVAILABLE,
    NOT_SIGNED_IN,
    OTHER_ACCOUNT,
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
    ENGINE_FILES,
    VOILE_FAILED,
    STEAM_COMPONENTS_FAILED,
    QUIT_FAILED,
    STILL_RUNNING,
    OTHER_SESSION,
    SETTINGS_FAILED,
];

pub const USAGE: &str = "usage";
pub const INTERNAL: &str = "internal";
pub const LIBRARY_UNAVAILABLE: &str = "library_unavailable";
pub const HELPERS_MISSING: &str = "helpers_missing";
pub const KEYCHAIN_UNAVAILABLE: &str = "keychain_unavailable";
pub const NOT_SIGNED_IN: &str = "not_signed_in";
pub const OTHER_ACCOUNT: &str = "other_account";
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
pub const ENGINE_FILES: &str = "engine_files";
pub const VOILE_FAILED: &str = "voile_failed";
pub const STEAM_COMPONENTS_FAILED: &str = "steam_components_failed";
pub const QUIT_FAILED: &str = "quit_failed";
pub const STILL_RUNNING: &str = "still_running";
pub const OTHER_SESSION: &str = "other_session";
pub const SETTINGS_FAILED: &str = "settings_failed";

/// Why a command failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    /// Obelisk's own reason, a localization key of its app such as
    /// `backend.steam.runtime.startFailed`, when the engine gave one.
    pub key: Option<String>,
    /// The engine files no game starts on, under `engine_files`.
    pub engine: Option<EngineFiles>,
}

/// Why no game starts on the Linux engine's files, and what ends it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct EngineFiles {
    /// The files that are not what they must be, by their path in the engine;
    /// none named when they are not known.
    pub paths: Vec<String>,
    pub reason: String,
    /// `repair`, `quit_game` or `check_storage`.
    pub action: String,
}

impl EngineFiles {
    /// What an engine reason tells, when it is one about the engine's files.
    fn read(key: &str, args: &[String]) -> Option<Self> {
        match (key, args) {
            (Incoherent::KEY, [reason, action, paths @ ..]) => Some(Self {
                paths: paths.to_vec(),
                reason: reason.clone(),
                action: action.clone(),
            }),
            // A library of the runtime is gone: which one the engine does not say.
            ("backend.runtime.wineIncomplete", _) => Some(Self {
                paths: Vec::new(),
                reason: "The runtime lacks a file a game cannot start without.".to_owned(),
                action: Action::Repair.name().to_owned(),
            }),
            // The runtime itself, or its Wine: a launch gets a runtime before
            // it is prepared, so what is missing here is a file of one.
            ("backend.runtime.wineUnavailable", _) => Some(Self {
                paths: Vec::new(),
                reason: "The runtime that runs Windows games is not there, or lacks its Wine.".to_owned(),
                action: Action::Repair.name().to_owned(),
            }),
            _ => None,
        }
    }
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            key: None,
            engine: None,
        }
    }

    /// A failure the engine reported as `reason`: the code that reason
    /// always means, else `otherwise`, the code of what was being done.
    pub fn engine(otherwise: &'static str, reason: impl AsRef<str>) -> Self {
        let (key, args, told) = read(reason.as_ref());
        let engine = EngineFiles::read(&key, &args);
        let code = known(&key).unwrap_or(otherwise);
        Self {
            code,
            message: format!("{}: {told}", sentence(code)),
            key: Some(key),
            engine,
        }
    }

    /// The process's exit status: 2 for a command line it could not read,
    /// else 1.
    pub fn status(&self) -> i32 {
        if self.code == USAGE { 2 } else { 1 }
    }
}

/// An engine reason's key and arguments, and the reason as a log tells it. The engine
/// gives a localization key, or a JSON object of a `key` and the `args` its
/// sentence takes, each of which may be such a reason too.
fn read(reason: &str) -> (String, Vec<String>, String) {
    #[derive(serde::Deserialize)]
    struct Message {
        key: String,
        #[serde(default)]
        args: Vec<String>,
    }
    match serde_json::from_str::<Message>(reason) {
        Ok(message) if message.args.is_empty() => (message.key.clone(), Vec::new(), message.key),
        Ok(message) => {
            let args: Vec<_> = message.args.iter().map(|arg| read(arg).2).collect();
            let told = format!("{} ({})", message.key, args.join(", "));
            (message.key, message.args, told)
        }
        Err(_) => (reason.to_owned(), Vec::new(), reason.to_owned()),
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
        // Whatever noticed it first.
        Incoherent::KEY | "backend.runtime.wineIncomplete" | "backend.runtime.wineUnavailable" => ENGINE_FILES,
        "backend.voile.inUse" => BUSY,
        "backend.voile.downloadFailed"
        | "backend.voile.damaged"
        | "backend.voile.noExchange"
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
        ENGINE_FILES => "the files that run Windows games cannot start one",
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

    /// Fifth verification, finding 5: a refusal over the engine's files is a
    /// code of its own, with every path and what ends it.
    #[test]
    fn the_engine_files_a_launch_is_refused_over_are_told() {
        let reason = serde_json::json!({
            "key": Incoherent::KEY,
            "args": ["the engine files are not all of one release", "repair", "engine/wsd", "a path, with a comma"]
        })
        .to_string();
        let failure = Failure::engine(LAUNCH_FAILED, &reason);
        assert_eq!(failure.code, ENGINE_FILES);
        let engine = failure.engine.unwrap();
        assert_eq!(engine.paths, ["engine/wsd", "a path, with a comma"]);
        assert_eq!(engine.reason, "the engine files are not all of one release");
        assert_eq!(engine.action, "repair");
        // Whichever probe noticed that a file is gone.
        for key in ["backend.runtime.wineIncomplete", "backend.runtime.wineUnavailable"] {
            let gone = Failure::engine(LAUNCH_FAILED, key);
            assert_eq!((gone.code, gone.engine.unwrap().action.as_str()), (ENGINE_FILES, "repair"), "{key}");
        }
        let disk = Failure::engine(
            LAUNCH_FAILED,
            game_runtime::voile::Failure::Exchange("/Volumes/a disk/Voile".into()).to_string(),
        );
        assert_eq!((disk.code, disk.engine), (VOILE_FAILED, None));
        assert!(disk.message.contains("/Volumes/a disk/Voile"), "{}", disk.message);
        assert_eq!(Failure::engine(LAUNCH_FAILED, "backend.voile.inUse").code, BUSY);
        assert_eq!(Failure::engine(LAUNCH_FAILED, "backend.steam.runtime.startFailed").engine, None);
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
