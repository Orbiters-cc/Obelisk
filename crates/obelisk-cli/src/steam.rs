//! The Steam commands, in the order Obelisk's app makes the same calls: the
//! saved session signs the account in, its Game sessions recover what a
//! previous run left, its installations open, and an action runs on them.
use crate::{
    arguments::{Guard, Login},
    credentials::{self, CredentialStore, Saved},
    events::{Event, GuardMethod, Sink},
    failure::{
        ALREADY_RUNNING, ANOTHER_GAME_RUNNING, BUSY, Failure, GAME_RUNNING, GUARD_REJECTED,
        GUARD_UNSUPPORTED, INPUT_MISSING, INTERNAL, LOGIN_FAILED, NO_WINDOWS_BUILD, NOT_INSTALLED,
        NOT_OWNED, NOT_RUNNING, OTHER_ACCOUNT, OTHER_SESSION, QUIT_FAILED, SESSION_RECOVERY_FAILED, SIGN_IN_EXPIRED,
        STEAM_FAILED, STILL_RUNNING,
    },
    follow::{self, Install, InstallStep, Play, PlayStep},
    places::Places,
};
use game_runtime::{installations::Receipt, launch_settings::HostFeatures, steam::Steam as SteamStore};
use obelisk_bridge::{
    AccountInstallations, CloudChoice, GameAccount, GameOffer, GameOpening, GamePlatform,
    GameSessionsState, Installation, OnlineStatus, SessionStorefront, SteamError, SteamGuardKind,
    SteamLogin, SteamSession, StorefrontSessions, VoileUpdateAnswer, installed_voile_release,
    steam_language,
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

/// How long the Game sessions get to tell what a previous run left.
const RECOVERY: Duration = Duration::from_secs(20);

/// One command's reach: its places, the saved session, and where it tells
/// its events.
pub struct Steam<'a> {
    pub places: &'a Places,
    pub store: &'a dyn CredentialStore,
    pub out: &'a dyn Sink,
}

/// How events name a confirmation Steam offers.
fn guard_name(kind: SteamGuardKind) -> &'static str {
    match kind {
        SteamGuardKind::None => "none",
        SteamGuardKind::EmailCode => Guard::EmailCode.name(),
        SteamGuardKind::DeviceCode => Guard::DeviceCode.name(),
        SteamGuardKind::DeviceConfirmation => Guard::DeviceConfirmation.name(),
        SteamGuardKind::EmailConfirmation => Guard::EmailConfirmation.name(),
        SteamGuardKind::MachineToken => "machine_token",
        SteamGuardKind::Unknown => "unknown",
    }
}

fn guard_kind(guard: Guard) -> SteamGuardKind {
    match guard {
        Guard::DeviceConfirmation => SteamGuardKind::DeviceConfirmation,
        Guard::DeviceCode => SteamGuardKind::DeviceCode,
        Guard::EmailCode => SteamGuardKind::EmailCode,
        Guard::EmailConfirmation => SteamGuardKind::EmailConfirmation,
    }
}

/// Whether Steam takes a typed code for `guard`.
fn needs_code(guard: Guard) -> bool {
    matches!(guard, Guard::DeviceCode | Guard::EmailCode)
}

/// The confirmation a sign-in uses among those Steam `offered`: the one
/// `asked` for, else the first the app would pick, the phone's approval
/// before a code. `None` when Steam asks for none.
fn choose_guard(
    offered: &[SteamGuardKind],
    asked: Option<Guard>,
) -> Result<Option<Guard>, Failure> {
    let offers = |guard: Guard| offered.iter().any(|kind| guard_name(*kind) == guard.name());
    let names = || {
        let names: Vec<_> = offered.iter().map(|kind| guard_name(*kind)).collect();
        names.join(", ")
    };
    if let Some(asked) = asked {
        return match offers(asked) {
            true => Ok(Some(asked)),
            false => Err(Failure::new(
                GUARD_UNSUPPORTED,
                format!(
                    "Steam does not offer {} for this sign-in; it offers: {}.",
                    asked.name(),
                    names()
                ),
            )),
        };
    }
    if let Some(guard) = Guard::ALL.into_iter().find(|guard| offers(*guard)) {
        return Ok(Some(guard));
    }
    let none = offered
        .iter()
        .all(|kind| matches!(kind, SteamGuardKind::None));
    match none {
        true => Ok(None),
        false => Err(Failure::new(
            GUARD_UNSUPPORTED,
            format!(
                "Steam asks for a confirmation this program cannot give: {}.",
                names()
            ),
        )),
    }
}

/// One line of standard input, without its end; none once it is closed.
async fn line() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(read) if read > 0 => Some(line.trim_end_matches(['\r', '\n']).to_owned()),
            _ => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// The locale the environment names, such as `fr_FR`; English without one.
fn locale() -> String {
    ["LC_ALL", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .unwrap_or_else(|| "en".into())
}

/// The process of the game of session `id`, as its supervisor's record in
/// `sessions` names it once the game runs.
pub(crate) fn recorded_pid(sessions: &Path, id: &str) -> Option<u32> {
    // A session is named by a UUID: nothing else names a file here.
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return None;
    }
    let bytes = std::fs::read(sessions.join(format!("{id}.v2.json"))).ok()?;
    let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    u32::try_from(record.get("game")?.get("pid")?.as_u64()?).ok()
}

/// The installations of the saved receipts file `file` whose folder is
/// there, read without signing in.
pub(crate) fn recorded_installations(file: &Path) -> Result<Vec<Receipt<SteamStore>>, Failure> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Failure::new(
                INTERNAL,
                format!("The installations cannot be read: {error}."),
            ));
        }
    };
    let receipts: Vec<Receipt<SteamStore>> = serde_json::from_slice(&bytes).map_err(|_| {
        Failure::new(
            INTERNAL,
            "The installations file cannot be read as receipts.",
        )
    })?;
    Ok(receipts
        .into_iter()
        .filter(|receipt| Path::new(&receipt.directory).is_dir())
        .collect())
}

/// Waits until the Game sessions told what a previous run left: a session
/// still live, or none.
async fn settled(sessions: &StorefrontSessions) -> Result<GameSessionsState, Failure> {
    let deadline = tokio::time::Instant::now() + RECOVERY;
    let mut state = sessions.state();
    while state.recovering {
        state = tokio::time::timeout_at(deadline, sessions.changed(state.revision))
            .await
            .map_err(|_| {
                Failure::new(
                    SESSION_RECOVERY_FAILED,
                    "The Game session a previous run left could not be reached: its record or the Steam host cannot be read.",
                )
            })?;
    }
    Ok(state)
}

impl Steam<'_> {
    fn tell(&self, event: Event) {
        self.out.emit(event);
    }

    /// `steam login`: the QR code or the password, the confirmation Steam
    /// asks for, then the session kept in the Keychain.
    pub async fn login(&self, login: Login) -> Result<(), Failure> {
        let failed = |error: SteamError| Failure::engine(LOGIN_FAILED, error.to_string());
        let (request, asked, by_password) = match login {
            Login::Qr => (SteamLogin::begin_qr().await.map_err(failed)?, None, false),
            Login::Account { name, guard } => {
                let password = line().await.filter(|password| !password.is_empty());
                let password = password.ok_or_else(|| {
                    Failure::new(
                        INPUT_MISSING,
                        "The password is read from the first line of standard input, which gave none.",
                    )
                })?;
                let request = SteamLogin::begin_credentials(name, password)
                    .await
                    .map_err(failed)?;
                (request, guard, true)
            }
        };
        let mut shown = None;
        let mut confirmed = !by_password;
        let credentials = loop {
            let challenge = request.challenge().await;
            if let Some(url) = challenge.qr_url.filter(|url| shown.as_ref() != Some(url)) {
                self.tell(Event::Challenge {
                    qr_url: url.clone(),
                });
                shown = Some(url);
            }
            if !confirmed {
                confirmed = true;
                let offered: Vec<_> = challenge.confirmations.iter().map(|m| m.kind).collect();
                if let Some(guard) = choose_guard(&offered, asked)? {
                    self.tell(Event::Guard {
                        kind: guard.name(),
                        needs_code: needs_code(guard),
                        methods: challenge
                            .confirmations
                            .iter()
                            .map(|method| GuardMethod {
                                kind: guard_name(method.kind),
                                message: method.message.clone(),
                            })
                            .collect(),
                    });
                    if needs_code(guard) {
                        let code = line().await.filter(|code| !code.trim().is_empty());
                        let code = code.ok_or_else(|| {
                            Failure::new(
                                INPUT_MISSING,
                                "The Steam Guard code is read from the next line of standard input, which gave none.",
                            )
                        })?;
                        request
                            .submit_guard(code.trim().to_owned(), guard_kind(guard))
                            .await
                            .map_err(|error| Failure::engine(GUARD_REJECTED, error.to_string()))?;
                    }
                }
            }
            let pause = u64::from(challenge.poll_interval_ms.max(1000));
            tokio::time::sleep(Duration::from_millis(pause)).await;
            if let Some(credentials) = request.poll().await.map_err(failed)? {
                break credentials;
            }
        };
        let saved = Saved::from(credentials);
        self.store.save(&saved).map_err(credentials::unavailable)?;
        let account = saved.account();
        self.tell(Event::SignedIn {
            steam_id: account.steam_id,
            account_name: account.account_name,
        });
        Ok(())
    }

    /// `steam logout`: the saved session goes, one this build may not read
    /// too. A game of the account that still runs keeps the session it
    /// started with, so it is quit first.
    pub async fn logout(&self) -> Result<(), Failure> {
        let saved = self.store.load();
        if let Ok(Some(saved)) = &saved
            && self.places.require_host().is_ok()
        {
            let (_, state) = self.sessions().await?;
            let playing = state
                .active
                .as_ref()
                .and_then(|active| follow::steam(&active.game))
                .is_some_and(|(_, account)| account == saved.steam_id);
            if playing {
                return Err(Failure::new(
                    GAME_RUNNING,
                    "A game of the account still runs with its session: quit it first.",
                ));
            }
        }
        self.store.remove().map_err(credentials::unavailable)?;
        self.tell(Event::SignedOut {
            was_signed_in: !matches!(saved, Ok(None)),
        });
        Ok(())
    }

    /// `steam installed`: the signed-in account's installations, read from
    /// its receipts without reaching Steam.
    pub fn installed(&self) -> Result<(), Failure> {
        let receipts = match self.store.load().map_err(credentials::unavailable)? {
            Some(saved) => recorded_installations(&self.places.receipts(saved.steam_id))?,
            None => Vec::new(),
        };
        let count = receipts.len();
        for receipt in receipts {
            self.tell(Event::Installation {
                app_id: receipt.product.app_id,
                name: receipt.name,
                directory: receipt.directory,
                platform: follow::platform(receipt.platform.into()),
                bytes: receipt.total_bytes,
                needs_verification: receipt.needs_verification,
            });
        }
        self.tell(Event::Done { count });
        Ok(())
    }

    /// Signs the saved account in. A session Steam refuses for good is
    /// forgotten, as nothing restores it.
    async fn connect(&self, saved: &Saved) -> Result<Arc<SteamSession>, Failure> {
        match SteamSession::connect(saved.clone().into(), None, OnlineStatus::Online).await {
            Ok(session) => Ok(session),
            Err(SteamError::AuthenticationRequired) => {
                let _ = self.store.remove();
                Err(Failure::new(
                    SIGN_IN_EXPIRED,
                    "Steam asks the account to sign in again: the saved session was forgotten. Run steam login.",
                ))
            }
            Err(error) => Err(Failure::engine(STEAM_FAILED, error.to_string())),
        }
    }

    /// Steam's Game sessions in this library, once they told what a previous
    /// run left.
    async fn sessions(&self) -> Result<(Arc<StorefrontSessions>, GameSessionsState), Failure> {
        self.places.require_host()?;
        let sessions = StorefrontSessions::new(SessionStorefront::Steam, self.places.locations());
        let state = settled(&sessions).await?;
        Ok((sessions, state))
    }

    /// The account's installations, as the app opens them at sign-in.
    fn installations(
        &self,
        saved: &Saved,
        session: &Arc<SteamSession>,
        sessions: &Arc<StorefrontSessions>,
    ) -> Result<Arc<AccountInstallations>, Failure> {
        AccountInstallations::steam(
            session.clone(),
            saved.clone().into(),
            sessions.clone(),
            self.places
                .receipts(saved.steam_id)
                .to_string_lossy()
                .into_owned(),
            self.places.locations(),
        )
        .map_err(|error| Failure::engine(INTERNAL, error.to_string()))
    }

    /// `steam install`: the game's Transfer, which gets the runtimes a launch
    /// needs before the game's files, and over an installation verifies it
    /// against Steam's current build and updates it in place.
    pub async fn install(&self, app_id: u32, language: Option<String>) -> Result<(), Failure> {
        let saved = credentials::signed_in(self.store)?;
        self.places.open_library()?;
        self.tell(Event::Preparing { app_id });
        let (sessions, _) = self.sessions().await?;
        let session = self.connect(&saved).await?;
        let installations = self.installations(&saved, &session, &sessions)?;
        let game = app_id.to_string();
        let language = steam_language(language.unwrap_or_else(locale));
        let details = session
            .details(app_id, language.clone())
            .await
            .map_err(|error| Failure::engine(STEAM_FAILED, error.to_string()))?;
        if details.platform.is_none() {
            return Err(Failure::new(
                NOT_OWNED,
                format!("The account can install no build of the game {app_id}."),
            ));
        }
        if details.windows_bytes == 0 {
            return Err(Failure::new(
                NO_WINDOWS_BUILD,
                format!(
                    "{} has no Windows build the account can install.",
                    details.name
                ),
            ));
        }
        let allowed = match installations.state().gate.modify {
            GameOpening::Closed => false,
            GameOpening::Open { except } => !except.contains(&game),
        };
        if !allowed {
            return Err(Failure::new(
                BUSY,
                "A game runs or another operation holds this one: nothing can be installed now.",
            ));
        }
        let offer = GameOffer::Steam {
            app_id,
            name: details.name,
            language,
        };
        installations.install(offer, Some(GamePlatform::Windows));
        let follow = Install::new(app_id);
        let mut state = installations.state();
        let installed = loop {
            let mut end = None;
            for step in follow.observe(&state, Instant::now()) {
                match step {
                    InstallStep::Tell(event) => self.tell(event),
                    InstallStep::End(outcome) => end = Some(outcome),
                }
            }
            if let Some(outcome) = end {
                break outcome;
            }
            state = installations.changed(state.revision).await;
        };
        session.shutdown();
        let Installation {
            name,
            directory,
            platform,
            total_bytes,
            ..
        } = installed?;
        self.tell(Event::Installed {
            app_id,
            name,
            directory,
            platform: follow::platform(platform),
            bytes: total_bytes,
            voile: installed_voile_release(self.places.locations()),
        });
        Ok(())
    }

    /// `steam play`: launches the installed Windows build as the app's Play
    /// does, with `arguments` after the game's own, and follows its Game
    /// session until it is finished. The supervisor owns the game: it runs on
    /// when this process ends. `steam_id` names the only account it may start
    /// for: the saved session read here, once, is the one it starts with.
    /// Without `saved_arguments`, `arguments` are the only ones of the
    /// player's: those kept in its launch settings start nothing.
    pub async fn play(
        &self,
        app_id: u32,
        steam_id: Option<u64>,
        arguments: Vec<String>,
        render_percent: Option<u32>,
        saved_arguments: bool,
        host: HostFeatures,
    ) -> Result<(), Failure> {
        let saved = credentials::signed_in(self.store)?;
        if steam_id.is_some_and(|expected| expected != saved.steam_id) {
            return Err(Failure::new(
                OTHER_ACCOUNT,
                "Another Steam account is signed in than the one the game was to start for: nothing was started.",
            ));
        }
        self.places.require_helpers()?;
        self.places.open_library()?;
        let (sessions, recovered) = self.sessions().await?;
        if let Some(active) = &recovered.active {
            let same = follow::steam(&active.game).is_some_and(|(game, _)| game == app_id);
            return Err(match same {
                true => Failure::new(
                    ALREADY_RUNNING,
                    format!("The game {app_id} already runs: see steam running."),
                ),
                false => Failure::new(
                    ANOTHER_GAME_RUNNING,
                    "Another Steam game runs, and Obelisk runs one at a time: see steam running.",
                ),
            });
        }
        self.tell(Event::Preparing { app_id });
        let session = self.connect(&saved).await?;
        let installations = self.installations(&saved, &session, &sessions)?;
        let game = app_id.to_string();
        let state = installations.state();
        let Some(installed) = follow::windows(&state.installations, &game) else {
            let stopped = follow::windows(&state.incomplete, &game).is_some();
            return Err(Failure::new(
                NOT_INSTALLED,
                match stopped {
                    true => format!(
                        "The download of the game {app_id} did not finish: run steam install."
                    ),
                    false => format!(
                        "The Windows build of the game {app_id} is not installed: run steam install."
                    ),
                },
            ));
        };
        let busy = || {
            Failure::new(
                BUSY,
                "Another operation holds the game: it cannot start now.",
            )
        };
        let allowed = match &state.gate.launch {
            GameOpening::Closed => false,
            GameOpening::Open { except } => !except.contains(&game),
        };
        if !allowed {
            return Err(busy());
        }
        // Read as its launch settings are, by this launch alone.
        if !arguments.is_empty() {
            game_runtime::launch_settings::add_arguments(arguments);
        }
        if !saved_arguments {
            game_runtime::launch_settings::ignore_saved_arguments();
        }
        if let Some(percent) = render_percent {
            game_runtime::launch_settings::override_render_percent(percent);
        }
        if host != HostFeatures::default() {
            game_runtime::launch_settings::ask_host_features(host);
        }
        let directory = installed.directory.clone();
        installations.launch(installed.id.clone());
        let admitted = installations.state();
        if admitted.launch_game.as_deref() != Some(&game) && admitted.report.is_none() {
            return Err(busy());
        }
        let records = self.places.steam_sessions();
        let mut follow = Play::new(app_id, saved.steam_id);
        let (mut installed, mut running) = (admitted, sessions.state());
        let outcome = loop {
            let steps = follow.observe(
                &installed,
                &running,
                |id| recorded_pid(&records, id),
                Instant::now(),
            );
            let mut end = None;
            for step in steps {
                match step {
                    PlayStep::Tell(event) => {
                        // By the profile its launch refreshed first.
                        if host != HostFeatures::default() && matches!(event, Event::Starting { .. }) {
                            self.tell(self.host_features(app_id, &directory));
                        }
                        self.tell(event);
                    }
                    PlayStep::TakeVoile { session } => {
                        let _ = sessions.answer_voile_update(session, VoileUpdateAnswer::Update);
                    }
                    PlayStep::DeclineRosetta {
                        session,
                        decision_id,
                    } => sessions.decline_rosetta(
                        session,
                        GameAccount::Steam {
                            steam_id: saved.steam_id,
                        },
                        decision_id,
                    ),
                    PlayStep::CancelCloud { decision_id } => {
                        let cancelled = installations
                            .answer_cloud(game.clone(), decision_id, CloudChoice::Cancel)
                            .await;
                        if cancelled.is_err() {
                            installations.cancel(game.clone());
                        }
                    }
                    PlayStep::End(outcome) => end = Some(outcome),
                }
            }
            if let Some(outcome) = end {
                break outcome;
            }
            tokio::select! {
                _ = installations.changed(installed.revision) => {}
                _ = sessions.changed(running.revision) => {}
            }
            // The sessions last: they settle before the launch lets go.
            installed = installations.state();
            running = sessions.state();
        };
        session.shutdown();
        outcome
    }

    /// What the launch of `app_id`, installed in `directory`, is given of
    /// what it asked the engine's host programs for: its own settings.
    fn host_features(&self, app_id: u32, directory: &str) -> Event {
        let game = game_runtime::profile::Game::Steam { app_id };
        let view = game_runtime::launch_settings::of(&self.places.library, game, Path::new(directory), None);
        let HostFeatures { free_pointer, shared_textures } = view.settings.host;
        Event::HostFeatures { app_id, free_pointer, shared_textures }
    }

    /// `steam running`: the Game session that holds Steam, if any.
    pub async fn running(&self) -> Result<(), Failure> {
        self.places.open_library()?;
        let (_, state) = self.sessions().await?;
        let records = self.places.steam_sessions();
        let live = state.active.as_ref().and_then(|active| {
            let (app_id, steam_id) = follow::steam(&active.game)?;
            Some(Event::Session {
                app_id,
                steam_id: steam_id.to_string(),
                session_id: active.id.clone(),
                phase: follow::phase(active.phase),
                pid: recorded_pid(&records, &active.id),
                reconnecting: active.reconnecting,
            })
        });
        let count = usize::from(live.is_some());
        live.into_iter().for_each(|event| self.tell(event));
        self.tell(Event::Done { count });
        Ok(())
    }

    /// `steam quit`: the app's Quit, which asks the game to close its
    /// windows and kills nothing, or with `force` its Force Quit; then, with
    /// `wait`, the game's exit within that many seconds.
    pub async fn quit(
        &self,
        app_id: u32,
        force: bool,
        wait: Option<u64>,
        session: Option<String>,
    ) -> Result<(), Failure> {
        self.places.open_library()?;
        let (sessions, state) = self.sessions().await?;
        let playing = |state: &GameSessionsState| {
            state
                .active
                .as_ref()
                .filter(|active| {
                    follow::steam(&active.game).is_some_and(|(game, _)| game == app_id)
                })
                .map(|active| active.id.clone())
        };
        let Some(id) = playing(&state) else {
            return Err(Failure::new(
                NOT_RUNNING,
                format!("The game {app_id} does not run."),
            ));
        };
        // A caller that names its own session never ends another one: a game
        // started meanwhile by someone else is left alone.
        if let Some(asked) = session.filter(|asked| *asked != id) {
            return Err(Failure::new(
                OTHER_SESSION,
                format!("The game {app_id} runs in the Game session {id}, not {asked}: nothing was asked to end."),
            ));
        }
        let asked = match force {
            true => sessions.force_quit(id.clone()).await,
            false => sessions.quit(id.clone()).await,
        };
        asked.map_err(|error| Failure::engine(QUIT_FAILED, error.to_string()))?;
        self.tell(Event::QuitRequested {
            app_id,
            forced: force,
        });
        let Some(wait) = wait else {
            return Ok(());
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
        let mut state = sessions.state();
        while playing(&state).as_ref() == Some(&id) {
            state = tokio::time::timeout_at(deadline, sessions.changed(state.revision))
                .await
                .map_err(|_| {
                    Failure::new(
                        STILL_RUNNING,
                        format!(
                            "The game {app_id} still runs {wait} seconds after it was asked to end."
                        ),
                    )
                })?;
        }
        let played = state
            .last
            .as_ref()
            .filter(|last| last.id == id)
            .and_then(|last| last.played.as_ref());
        self.tell(Event::Exited {
            app_id,
            session_id: Some(id.clone()),
            frames: played.map(|played| played.frames),
            exit_code: played.and_then(|played| played.exit_code),
            exit_signal: played.and_then(|played| played.exit_signal),
            seconds: played.map(|played| played.seconds),
            ended_by_request: played.is_none_or(|played| played.ended_by_obelisk),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
