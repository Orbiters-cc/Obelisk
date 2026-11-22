//! What a command tells the program that runs it: JSON Lines on standard
//! output, one object per line, each with its `event`. Nothing else is ever
//! written there. docs/CLI.md lists every event and its fields.
use crate::failure::Failure;
use serde::Serialize;
use std::{
    io::Write,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Every event's name. A test checks docs/CLI.md describes each.
#[cfg(test)]
pub const NAMES: &[&str] = &[
    "version",
    "status",
    "challenge",
    "guard",
    "signed_in",
    "signed_out",
    "installation",
    "done",
    "preparing",
    "progress",
    "voile_update",
    "cloud_pending",
    "installed",
    "starting",
    "running",
    "exited",
    "session",
    "quit_requested",
    "settings",
    "error",
];

/// A Steam account, without its session. The Steam ID is text: it does not
/// fit the numbers every JSON reader keeps exactly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Account {
    pub steam_id: String,
    pub account_name: String,
}

/// Where the Steam helpers stand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Helpers {
    pub directory: String,
    /// Every helper a launch needs is there.
    pub found: bool,
    pub missing: Vec<String>,
    /// The Intel helper app that tells whether Rosetta is installed.
    pub rosetta_helper: bool,
}

/// The Voile runtime Windows games run in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Voile {
    pub installed: Option<String>,
    pub prereleases: bool,
    /// Which keyboard the games of the Linux engine get: `mac`, `pending`
    /// or `us`. None without a Linux engine.
    pub keyboard_layout: Option<&'static str>,
}

/// One way Steam offers to confirm a sign-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuardMethod {
    pub kind: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Version {
        version: &'static str,
    },
    Status {
        version: &'static str,
        library: String,
        account: Option<Account>,
        /// `ok`, or `unavailable` when the Keychain could not be read.
        keychain: &'static str,
        voile: Voile,
        helpers: Helpers,
    },
    Challenge {
        qr_url: String,
    },
    Guard {
        /// The confirmation the sign-in waits for.
        kind: &'static str,
        /// A code is read from standard input for it.
        needs_code: bool,
        methods: Vec<GuardMethod>,
    },
    SignedIn {
        steam_id: String,
        account_name: String,
    },
    SignedOut {
        /// An account was signed in.
        was_signed_in: bool,
    },
    Installation {
        app_id: u32,
        name: String,
        directory: String,
        platform: &'static str,
        bytes: u64,
        needs_verification: bool,
    },
    Done {
        count: usize,
    },
    Preparing {
        app_id: u32,
    },
    Progress {
        app_id: u32,
        phase: &'static str,
        bytes_done: Option<u64>,
        bytes_total: Option<u64>,
        fraction: Option<f64>,
    },
    VoileUpdate {
        app_id: u32,
        version: String,
        required: bool,
        /// Always `update`: the launch takes the release.
        answer: &'static str,
    },
    CloudPending {
        app_id: u32,
        /// `conflict` or `failed`.
        kind: &'static str,
    },
    Installed {
        app_id: u32,
        name: String,
        directory: String,
        platform: &'static str,
        bytes: u64,
        voile: Option<String>,
    },
    Starting {
        app_id: u32,
    },
    Running {
        app_id: u32,
        pid: Option<u32>,
        /// The Game session that holds the game.
        session_id: Option<String>,
    },
    Exited {
        app_id: u32,
        /// The Game session that held the game.
        session_id: Option<String>,
        /// How many frames the game presented, as its session counted them.
        frames: Option<u64>,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
        seconds: Option<u64>,
        /// The game was asked to end through Obelisk.
        ended_by_request: bool,
    },
    Session {
        app_id: u32,
        steam_id: String,
        session_id: String,
        phase: &'static str,
        pid: Option<u32>,
        reconnecting: bool,
    },
    QuitRequested {
        app_id: u32,
        forced: bool,
    },
    Settings {
        prereleases: bool,
    },
    Error {
        code: &'static str,
        message: String,
        key: Option<String>,
    },
}

impl From<&Failure> for Event {
    fn from(failure: &Failure) -> Self {
        Self::Error {
            code: failure.code,
            message: failure.message.clone(),
            key: failure.key.clone(),
        }
    }
}

impl Event {
    /// The event as its line, without the line's end.
    pub fn line(&self) -> String {
        serde_json::to_string(self).expect("an event is plain data")
    }
}

/// Where a command's events go.
pub trait Sink: Send + Sync {
    fn emit(&self, event: Event);
}

/// Standard output, one event per line, each written whole and at once. A
/// reader that went away loses the events and stops nothing: the game a
/// command started still runs.
pub struct Stdout;

impl Sink for Stdout {
    fn emit(&self, event: Event) {
        let mut output = std::io::stdout().lock();
        let _ = writeln!(output, "{}", event.line());
        let _ = output.flush();
    }
}

/// The events a test's command told, in order.
#[cfg(test)]
#[derive(Default)]
pub struct Told(pub Mutex<Vec<Event>>);

#[cfg(test)]
impl Sink for Told {
    fn emit(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

#[cfg(test)]
impl Told {
    pub fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
}

/// How often a phase tells its progress.
pub const PACE: Duration = Duration::from_millis(250);

/// Lets a phase's progress through a few times per second: its first, then
/// one every `PACE`. Another phase starts at once.
#[derive(Default)]
pub struct Throttle {
    last: Mutex<Option<(&'static str, Instant)>>,
}

impl Throttle {
    /// Whether progress of `phase` is told at `now`.
    pub fn passes(&self, phase: &'static str, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap();
        let due = match *last {
            Some((told, at)) => told != phase || now.saturating_duration_since(at) >= PACE,
            None => true,
        };
        if due {
            *last = Some((phase, now));
        }
        due
    }
}

/// The share done of `total`, when there is one.
pub fn fraction(done: u64, total: u64) -> Option<f64> {
    (total > 0).then(|| (done as f64 / total as f64).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure;

    #[test]
    fn an_event_is_one_json_object_on_one_line_named_by_its_event() {
        let line = Event::Running {
            app_id: 438100,
            pid: Some(4242),
            session_id: Some("f2931045-90d3-4302-a8a9-94021747c54f".into()),
        }
        .line();
        assert_eq!(
            line,
            r#"{"event":"running","app_id":438100,"pid":4242,"session_id":"f2931045-90d3-4302-a8a9-94021747c54f"}"#
        );
        assert!(!line.contains('\n'));
        let progress = Event::Progress {
            app_id: 438100,
            phase: "voile",
            bytes_done: None,
            bytes_total: None,
            fraction: Some(0.5),
        };
        assert_eq!(
            progress.line(),
            r#"{"event":"progress","app_id":438100,"phase":"voile","bytes_done":null,"bytes_total":null,"fraction":0.5}"#
        );
    }

    #[test]
    fn a_steam_id_crosses_as_text_and_a_name_with_a_line_break_stays_on_its_line() {
        let line = Event::SignedIn {
            steam_id: 76_561_197_960_265_729_u64.to_string(),
            account_name: "two\nlines".into(),
        }
        .line();
        assert_eq!(
            line,
            r#"{"event":"signed_in","steam_id":"76561197960265729","account_name":"two\nlines"}"#
        );
    }

    #[test]
    fn a_failure_is_an_error_event_with_its_code_message_and_reason() {
        let failure = Failure::engine(failure::LAUNCH_FAILED, "backend.steam.runtime.startFailed");
        assert_eq!(
            Event::from(&failure).line(),
            r#"{"event":"error","code":"launch_failed","message":"the launch failed: backend.steam.runtime.startFailed","key":"backend.steam.runtime.startFailed"}"#
        );
        let plain = Failure::new(failure::NOT_SIGNED_IN, "No Steam account is signed in.");
        assert_eq!(
            Event::from(&plain).line(),
            r#"{"event":"error","code":"not_signed_in","message":"No Steam account is signed in.","key":null}"#
        );
    }

    #[test]
    fn progress_passes_a_few_times_per_second_and_at_once_for_another_phase() {
        let throttle = Throttle::default();
        let start = Instant::now();
        assert!(throttle.passes("downloading", start));
        assert!(!throttle.passes("downloading", start + Duration::from_millis(100)));
        assert!(!throttle.passes("downloading", start + Duration::from_millis(249)));
        assert!(throttle.passes("downloading", start + Duration::from_millis(250)));
        assert!(throttle.passes("verifying", start + Duration::from_millis(260)));
        assert!(!throttle.passes("verifying", start + Duration::from_millis(300)));
    }

    #[test]
    fn a_fraction_needs_a_total_and_never_passes_the_whole() {
        assert_eq!(fraction(1, 0), None);
        assert_eq!(fraction(1, 4), Some(0.25));
        assert_eq!(fraction(9, 4), Some(1.0));
    }

    #[test]
    fn every_event_is_named_once_and_described_in_the_manual() {
        let manual = include_str!("../../../docs/CLI.md");
        for (index, name) in NAMES.iter().enumerate() {
            assert!(!NAMES[..index].contains(name), "{name} is listed twice");
            assert!(
                manual.contains(&format!("### `{name}`")),
                "docs/CLI.md does not describe the event {name}"
            );
        }
        // Every variant is one of the names.
        let sample = [
            Event::Version { version: "0" }.line(),
            Event::Done { count: 0 }.line(),
            Event::Preparing { app_id: 1 }.line(),
            Event::Starting { app_id: 1 }.line(),
            Event::Settings { prereleases: true }.line(),
            Event::SignedOut {
                was_signed_in: false,
            }
            .line(),
            Event::QuitRequested {
                app_id: 1,
                forced: false,
            }
            .line(),
            Event::CloudPending {
                app_id: 1,
                kind: "conflict",
            }
            .line(),
        ];
        for line in sample {
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            let name = value["event"].as_str().unwrap();
            assert!(NAMES.contains(&name), "{name}");
        }
    }
}
