//! What a command makes of the engine's state as it changes: the events it
//! tells, the questions it answers in the player's place, and how it ends.
//! Nothing here reaches Steam or a process, so tests drive it with states.
//!
//! Where Obelisk's app asks its player, a command answers at once and says
//! so: it takes a newer Voile release, declines to install Rosetta, and
//! cancels a Cloud sync that asks which saves to keep.
use crate::{
    events::{Event, Throttle, fraction},
    failure::{
        BUSY, CANCELLED, CLOUD_CONFLICT, CLOUD_SYNC_FAILED, Failure, INSTALL_FAILED, INTERNAL,
        LAUNCH_FAILED, ROSETTA_REQUIRED, SESSION_FAILED,
    },
};
use obelisk_bridge::{
    CloudQuestionKind, CloudSyncKind, GameAccount, GameIdentity, GamePhase, GamePlatform,
    GameSessionState, GameSessionsState, GameTransfer, Installation, InstallationReport,
    InstallationsState, RuntimeComponent, TransferState,
};
use std::time::Instant;

/// What a Transfer's progress says while a runtime downloads for it.
const INSTALLING_VOILE: &str = "model.game.installingVoile";
const INSTALLING_STEAM_COMPONENTS: &str = "model.game.installingSteamComponents";

/// How events and the manual name a build.
pub fn platform(platform: GamePlatform) -> &'static str {
    match platform {
        GamePlatform::Mac => "mac",
        GamePlatform::Windows => "windows",
    }
}

/// How events and the manual name a Game session's phase.
pub fn phase(phase: GamePhase) -> &'static str {
    match phase {
        GamePhase::Preparing => "preparing",
        GamePhase::Running => "running",
        GamePhase::Finalizing => "finalizing",
        GamePhase::Finished => "finished",
        GamePhase::Failed => "failed",
    }
}

/// The Steam game and account a session names.
pub fn steam(game: &GameIdentity) -> Option<(u32, u64)> {
    match game {
        GameIdentity::Steam { app_id, account_id } => Some((*app_id, *account_id)),
        _ => None,
    }
}

/// A Transfer under way as a `progress` event; none once it settled.
fn progress(app_id: u32, transfer: &GameTransfer) -> Option<(&'static str, Event)> {
    let phase = match (transfer.state, transfer.message.as_str()) {
        (TransferState::Downloading, INSTALLING_VOILE) => "voile",
        (TransferState::Downloading, INSTALLING_STEAM_COMPONENTS) => "steam_components",
        (TransferState::Preparing, _) => "preparing",
        (TransferState::Verifying, _) => "verifying",
        (TransferState::Downloading, _) => "downloading",
        (TransferState::Moving, _) => "moving",
        (TransferState::Complete | TransferState::Cancelled | TransferState::Failed, _) => {
            return None;
        }
    };
    let event = Event::Progress {
        app_id,
        phase,
        bytes_done: Some(transfer.completed_bytes),
        bytes_total: Some(transfer.total_bytes),
        fraction: fraction(transfer.completed_bytes, transfer.total_bytes),
    };
    Some((phase, event))
}

/// A failure the installations show, as a command's.
fn reported(otherwise: &'static str, report: &InstallationReport) -> Failure {
    match report {
        InstallationReport::Storefront { key } => Failure::engine(otherwise, key),
        InstallationReport::Uninstall { reason } | InstallationReport::Move { reason } => {
            Failure::engine(otherwise, reason)
        }
        InstallationReport::DownloadDirectory { message } => Failure::new(
            otherwise,
            format!("The games folder could not be made: {message}."),
        ),
        InstallationReport::GamesFolderUnavailable { path } => Failure::new(
            otherwise,
            format!("The games folder {path} is not there: its disk is not connected."),
        ),
    }
}

/// The Windows installation of the game Rust names `game`.
pub fn windows<'a>(installations: &'a [Installation], game: &str) -> Option<&'a Installation> {
    installations
        .iter()
        .find(|installed| installed.game == game && installed.platform == GamePlatform::Windows)
}

/// What following an install does next.
#[derive(Clone, Debug, PartialEq)]
pub enum InstallStep {
    Tell(Event),
    /// The Transfer ended: with the installation, or why it has none.
    End(Result<Installation, Failure>),
}

/// One `steam install`, from its Transfer joining the download queue to its
/// installation.
pub struct Install {
    app_id: u32,
    game: String,
    throttle: Throttle,
}

impl Install {
    pub fn new(app_id: u32) -> Self {
        Self {
            app_id,
            game: app_id.to_string(),
            throttle: Throttle::default(),
        }
    }

    /// What `state` tells of the game's Transfer at `now`.
    pub fn observe(&self, state: &InstallationsState, now: Instant) -> Vec<InstallStep> {
        let ours = state.transfer_game.as_deref() == Some(&self.game);
        let waiting = state.queued.iter().any(|queued| queued.game == self.game);
        let failed = |otherwise| {
            state
                .report
                .as_ref()
                .map(|report| reported(otherwise, report))
        };
        if ours && state.downloading {
            let told = state
                .transfer
                .as_ref()
                .and_then(|transfer| progress(self.app_id, transfer))
                .filter(|(phase, _)| self.throttle.passes(phase, now));
            return told
                .map(|(_, event)| InstallStep::Tell(event))
                .into_iter()
                .collect();
        }
        if waiting {
            return Vec::new();
        }
        let settled = state.transfer.as_ref().filter(|_| ours);
        let end = match settled.map(|transfer| (transfer.state, transfer.message.as_str())) {
            Some((TransferState::Complete, _)) => windows(&state.installations, &self.game)
                .cloned()
                .ok_or_else(|| {
                    Failure::new(
                        INTERNAL,
                        "The download completed without a Windows installation.",
                    )
                }),
            Some((TransferState::Cancelled, _)) => {
                Err(Failure::new(CANCELLED, "The download was cancelled."))
            }
            Some((TransferState::Failed, message)) => match failed(INSTALL_FAILED) {
                Some(failure) => Err(failure),
                None if !message.is_empty() => Err(Failure::engine(INSTALL_FAILED, message)),
                // Its report follows.
                None => return Vec::new(),
            },
            // It settles next.
            Some(_) => return Vec::new(),
            // The Transfer never joined the queue.
            None => Err(failed(INSTALL_FAILED).unwrap_or_else(|| {
                Failure::new(
                    BUSY,
                    "Another operation holds the game: nothing was installed.",
                )
            })),
        };
        vec![InstallStep::End(end)]
    }
}

/// What following a launch does next.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayStep {
    Tell(Event),
    /// Takes the newer Voile release the launch of `session` offers.
    TakeVoile {
        session: String,
    },
    /// Declines to install Rosetta: the waiting launch stops.
    DeclineRosetta {
        session: String,
        decision_id: u64,
    },
    /// Cancels the Cloud sync that asks its question `decision_id`.
    CancelCloud {
        decision_id: u64,
    },
    /// The launch ended: its game ran and exited, or why it did not.
    End(Result<(), Failure>),
}

/// One `steam play`, from its admission until its Game session is finished.
pub struct Play {
    app_id: u32,
    steam_id: u64,
    game: String,
    throttle: Throttle,
    starting: bool,
    running: bool,
    /// This launch's own Game session, from the first time its supervisor is seen (before it runs): only its own
    /// record says how it ended.
    session: Option<String>,
    /// When this launch was asked for (seconds since 1970): a session that began before it is never this launch's.
    admitted_at: u64,
    /// The session and release of the Voile offer already taken.
    voile: Option<(String, String)>,
    rosetta: Option<u64>,
    cloud: Option<u64>,
    /// Why the launch stops, once an answer given in the player's place
    /// stopped it.
    failure: Option<Failure>,
}

impl Play {
    pub fn new(app_id: u32, steam_id: u64) -> Self {
        Self {
            app_id,
            steam_id,
            game: app_id.to_string(),
            throttle: Throttle::default(),
            starting: false,
            running: false,
            session: None,
            admitted_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            voile: None,
            rosetta: None,
            cloud: None,
            failure: None,
        }
    }

    fn ours(&self, session: &GameSessionState) -> bool {
        steam(&session.game) == Some((self.app_id, self.steam_id))
    }

    /// What the installations and the Game sessions tell of the launch at
    /// `now`. `pid` reads the process of a session's game.
    pub fn observe(
        &mut self,
        installations: &InstallationsState,
        sessions: &GameSessionsState,
        pid: impl Fn(&str) -> Option<u32>,
        now: Instant,
    ) -> Vec<PlayStep> {
        let app_id = self.app_id;
        let mut steps = Vec::new();
        // The Transfer a launch runs first, to repair or update the game.
        if installations.launching
            && installations.verifying_before_launch
            && installations.transfer_game.as_deref() == Some(&self.game)
            && let Some(transfer) = &installations.transfer
            && let Some((phase, event)) = progress(app_id, transfer)
            && self.throttle.passes(phase, now)
        {
            steps.push(PlayStep::Tell(event));
        }
        if let Some(active) = sessions.active.as_ref().filter(|active| self.ours(active)) {
            // This launch's session once its supervisor took it, whatever phase it is in: until then it has a name of
            // this process's own.
            if active.observed
                && self.session.is_none()
                && active.started_at.is_none_or(|at| at + 2 >= self.admitted_at)
            {
                self.session = Some(active.id.clone());
            }
            if let Some(install) = active.runtime_install {
                let phase = match install.component {
                    RuntimeComponent::Voile => "voile",
                    RuntimeComponent::SteamComponents => "steam_components",
                };
                if self.throttle.passes(phase, now) {
                    steps.push(PlayStep::Tell(Event::Progress {
                        app_id,
                        phase,
                        bytes_done: None,
                        bytes_total: None,
                        fraction: Some(install.fraction),
                    }));
                }
            }
            if let Some(offer) = &active.voile_update {
                let taken = (active.id.clone(), offer.version.clone());
                if self.voile.as_ref() != Some(&taken) {
                    self.voile = Some(taken);
                    steps.push(PlayStep::Tell(Event::VoileUpdate {
                        app_id,
                        version: offer.version.clone(),
                        required: offer.required,
                        answer: "update",
                    }));
                    steps.push(PlayStep::TakeVoile {
                        session: active.id.clone(),
                    });
                }
            }
            if active.observed && !self.starting {
                self.starting = true;
                steps.push(PlayStep::Tell(Event::Starting {
                    app_id,
                    session_id: self.session.clone(),
                }));
            }
            if active.phase == GamePhase::Running && !self.running {
                self.running = true;
                self.session = Some(active.id.clone());
                steps.push(PlayStep::Tell(Event::Running {
                    app_id,
                    pid: pid(&active.id),
                    session_id: Some(active.id.clone()),
                }));
            }
        }
        if let Some(prompt) = &sessions.rosetta
            && prompt.account
                == (GameAccount::Steam {
                    steam_id: self.steam_id,
                })
            && self.rosetta != Some(prompt.decision_id)
        {
            self.rosetta = Some(prompt.decision_id);
            self.failure = Some(Failure::new(
                ROSETTA_REQUIRED,
                "The game needs Rosetta, which is not installed: install it with softwareupdate --install-rosetta.",
            ));
            steps.push(PlayStep::DeclineRosetta {
                session: prompt.session_id.clone(),
                decision_id: prompt.decision_id,
            });
        }
        if let Some(sync) = &installations.cloud_sync
            && sync.game == self.game
            && let Some(question) = &sync.question
            && self.cloud != Some(question.decision_id)
        {
            self.cloud = Some(question.decision_id);
            let (kind, code, sentence) = match question.kind {
                CloudQuestionKind::Conflict => (
                    "conflict",
                    CLOUD_CONFLICT,
                    "The saves on this Mac and in Steam Cloud differ, and only the player can choose which to keep",
                ),
                CloudQuestionKind::Failed => (
                    "failed",
                    CLOUD_SYNC_FAILED,
                    "Steam Cloud could not be checked before the game starts",
                ),
            };
            if sync.kind == CloudSyncKind::Launch {
                let reason = question.error.as_deref().unwrap_or("no reason given");
                self.failure = Some(Failure {
                    code,
                    message: format!("{sentence}: {reason}."),
                    key: question.error.clone(),
                    engine: None,
                });
            } else {
                steps.push(PlayStep::Tell(Event::CloudPending { app_id, kind }));
            }
            steps.push(PlayStep::CancelCloud {
                decision_id: question.decision_id,
            });
        }
        // The launch holds its game until its Game session is finished.
        if !installations.launching && installations.launch_game.as_deref() != Some(&self.game) {
            steps.extend(self.ended(installations, sessions));
        }
        steps
    }

    /// How the launch ended: its game's exit when it ran, then its failure.
    fn ended(
        &mut self,
        installations: &InstallationsState,
        sessions: &GameSessionsState,
    ) -> Vec<PlayStep> {
        let app_id = self.app_id;
        let mut steps = Vec::new();
        // A short game may show only there, already finished. Once its
        // session was seen running, only that session's record is its: another
        // launch of the game in the library may have become the last since.
        let last = sessions.last.as_ref().filter(|last| {
            self.ours(last)
                && last.observed
                && match &self.session {
                    Some(id) => *id == last.id,
                    // Never seen: only a session that began after this launch was asked for can be its.
                    None => last.started_at.is_some_and(|at| at + 2 >= self.admitted_at),
                }
        });
        let played = last.and_then(|last| last.played.as_ref());
        let ran =
            self.running || played.is_some() || last.is_some_and(|last| last.started_at.is_some());
        if ran {
            if !self.starting {
                steps.push(PlayStep::Tell(Event::Starting {
                    app_id,
                    session_id: last.map(|last| last.id.clone()),
                }));
            }
            if !self.running {
                steps.push(PlayStep::Tell(Event::Running {
                    app_id,
                    pid: None,
                    session_id: last.map(|last| last.id.clone()),
                }));
            }
            steps.push(PlayStep::Tell(Event::Exited {
                app_id,
                session_id: self
                    .session
                    .clone()
                    .or_else(|| last.map(|last| last.id.clone())),
                // Unknown, not 0, when the game's log could not be read.
                frames: played
                    .filter(|played| played.frames_measured)
                    .map(|played| played.frames),
                exit_code: played.and_then(|played| played.exit_code),
                exit_signal: played.and_then(|played| played.exit_signal),
                seconds: played.map(|played| played.seconds),
                ended_by_request: played.is_some_and(|played| played.ended_by_obelisk),
            }));
        }
        let after = if ran { SESSION_FAILED } else { LAUNCH_FAILED };
        let end = match (self.failure.take(), &installations.report) {
            (Some(failure), _) => Err(failure),
            (None, Some(report)) => Err(reported(after, report)),
            (None, None) if !ran => Err(Failure::new(
                CANCELLED,
                "The launch was cancelled before the game started.",
            )),
            (None, None) => Ok(()),
        };
        steps.push(PlayStep::End(end));
        steps
    }
}

#[cfg(test)]
mod tests;
