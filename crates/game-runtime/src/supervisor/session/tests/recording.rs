//! A recording storefront: the core's lifecycle with nothing else around it.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Recording;
/// The same storefront, waiting for its UI's Begin.
pub(crate) struct Gated;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Game {
    name: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Failure {
    #[error("fixture.cancelled")]
    Cancelled,
    #[error("fixture.{0:?}")]
    Core(Error),
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        match error {
            Error::Cancelled => Self::Cancelled,
            error => Self::Core(error),
        }
    }
}

impl Provider for Recording {
    type Account = u64;
    type Store = Game;
    type Control = crate::supervisor::NoControl;
    type Error = Failure;
    const SUPERVISE: &'static str = "unused";
    const CANCELLED: &'static [&'static str] = &["fixture.cancelled"];

    fn valid(_: &Record<Self>) -> bool {
        true
    }
    fn cancelled(error: &Failure) -> bool {
        matches!(error, Failure::Cancelled)
    }
    fn same_game(a: &Game, b: &Game) -> bool {
        a == b
    }
    fn orphaned(_: &mut Record<Self>, _: bool) {}
}

const ACCOUNT: u64 = 7;

#[derive(Serialize, Deserialize)]
struct Request {
    sessions: PathBuf,
    directory: PathBuf,
    id: String,
}

fn handoff(
    directory: &Path,
    sessions: &Path,
    id: &str,
    pipe: &mut std::os::unix::net::UnixStream,
) -> Result<(), Error> {
    storage::send(
        pipe,
        &Request {
            sessions: sessions.into(),
            directory: directory.into(),
            id: id.into(),
        },
    )
}

impl Harness for Recording {
    type P = Recording;
    const NAME: &'static str = "recording";
    fn account() -> u64 {
        ACCOUNT
    }
    fn other_account() -> u64 {
        ACCOUNT + 1
    }
    fn handoff(
        directory: &Path,
        sessions: &Path,
        id: &str,
        pipe: &mut std::os::unix::net::UnixStream,
    ) -> Result<(), Error> {
        handoff(directory, sessions, id, pipe)
    }
    fn serve(
        input: std::io::StdinLock<'static>,
        output: std::io::StderrLock<'static>,
    ) -> Result<(), String> {
        serve::<Recorder<false>>(input, output, inspect).map_err(|error| format!("{error:?}"))
    }
    fn previous_build_record(id: &str, live: &Identity, socket: &Path) -> serde_json::Value {
        serde_json::json!({
            "id": id, "account_id": ACCOUNT, "phase": "Running", "revision": 3,
            "supervisor": live, "game": live, "socket": socket,
        })
    }
}

impl Harness for Gated {
    type P = Recording;
    const NAME: &'static str = "gated";
    fn account() -> u64 {
        ACCOUNT
    }
    fn other_account() -> u64 {
        ACCOUNT + 1
    }
    fn handoff(
        directory: &Path,
        sessions: &Path,
        id: &str,
        pipe: &mut std::os::unix::net::UnixStream,
    ) -> Result<(), Error> {
        handoff(directory, sessions, id, pipe)
    }
    fn serve(
        input: std::io::StdinLock<'static>,
        output: std::io::StderrLock<'static>,
    ) -> Result<(), String> {
        serve::<Recorder<true>>(input, output, inspect).map_err(|error| format!("{error:?}"))
    }
    fn previous_build_record(id: &str, live: &Identity, socket: &Path) -> serde_json::Value {
        serde_json::json!({
            "id": id, "account_id": ACCOUNT, "phase": "Running", "revision": 3,
            "supervisor": live, "game": live, "socket": socket,
        })
    }
}

struct Recorder<const BEGIN: bool> {
    directory: PathBuf,
    script: Script,
}

impl<const BEGIN: bool> Launch for Recorder<BEGIN> {
    type P = Recording;
    type Host = ();
    type Cloud = Cloud;
    const BEGIN: bool = BEGIN;

    fn open(input: &mut dyn Read) -> Result<(Opening<Recording>, Self), Failure> {
        let request: Request = storage::packet(input)?;
        let (directory, script) = fixture_directory();
        write_helper(&directory.join("helper"), &directory, &script);
        Ok((
            Opening {
                root: request.sessions,
                id: request.id,
                account_id: ACCOUNT,
                store: Game {
                    name: "fixture".into(),
                },
                helpers: directory.clone(),
                status: obelisk_core::OnlineStatus::default(),
            },
            Self { directory, script },
        ))
    }

    fn wine(&self) -> Option<crate::wine::Config> {
        self.script.wine.clone()
    }

    async fn authenticate(&mut self, _: &Arc<Session<Recording>>) -> Result<(), Failure> {
        record(&self.directory, "authenticate");
        Ok(())
    }

    async fn cloud(&mut self, session: &Arc<Session<Recording>>) -> Result<Option<Cloud>, Failure> {
        Ok(Some(Cloud {
            directory: self.directory.clone(),
            script: self.script.clone(),
            session: session.clone(),
        }))
    }

    async fn target(
        &mut self,
        _: &Arc<Session<Recording>>,
    ) -> Result<(Target, Option<Architecture>), Failure> {
        record(&self.directory, "target");
        Ok((
            target(&self.directory, &self.script),
            self.script.intel.then_some(Architecture::Intel),
        ))
    }

    async fn prepare_host(
        &mut self,
        _: &Arc<Session<Recording>>,
        target: &Target,
        _: Option<&Cloud>,
    ) -> Result<Prepared<()>, Failure> {
        record(&self.directory, "host.prepare");
        Ok(Prepared {
            host: (),
            helper: self.directory.join("helper"),
            environment: target.environment.clone(),
        })
    }

    fn running(&self, _: &Arc<()>, _: u32) -> crate::supervisor::session::Publish {
        let directory = self.directory.clone();
        Box::new(move |shown| {
            record(
                &directory,
                &crate::supervisor::session::presence_event(shown),
            )
        })
    }

    fn stopped(&self, _: &(), _: Option<i32>) {}

    fn rosetta(helper: &Path) -> Result<bool, Error> {
        Ok(helper.exists())
    }
}

mod lifecycle_of_the_core {
    super::super::lifecycle_suite!(super::Recording);
    super::super::rosetta_suite!(super::Recording);
}

#[test]
fn begin_opens_the_start_gate_once_and_only_for_its_account() {
    super::begin_opens_the_start_gate_once_and_only_for_its_account::<Gated>();
}

/// The recorded Cloud sync, which follows the test's script.
pub(crate) struct Cloud {
    directory: PathBuf,
    script: Script,
    session: Arc<Session<Recording>>,
}

impl LaunchCloud<Failure> for Cloud {
    fn prepare<'a>(&'a mut self, _: &'a SessionAlive<'a>) -> Operation<'a, Result<(), Failure>> {
        Box::pin(async move {
            record(&self.directory, "cloud.prepare");
            if self.script.hold_cloud {
                fs::write(self.directory.join("cloud-ready"), b"").unwrap();
                hold(&self.directory, "release-cloud", || self.session.check()).await?;
            }
            if self.script.ask {
                let cloud = self.session.cloud();
                let answer = cloud
                    .ask(crate::cloud::Question::Conflict, None)
                    .await
                    .map_err(|_| Failure::Cancelled)?;
                record(&self.directory, &format!("answered:{answer:?}"));
                cloud.show(crate::cloud::Phase::Ready, None);
            }
            Ok(())
        })
    }
    fn started(&mut self) -> Result<(), Failure> {
        record(&self.directory, "cloud.started");
        Ok(())
    }
    fn finished(&mut self, confirmed: bool) -> Operation<'_, ()> {
        Box::pin(async move {
            if self.script.hold_finish {
                fs::write(self.directory.join("finish-ready"), b"").unwrap();
                let _ = hold(&self.directory, "release-finish", || Ok::<_, Error>(())).await;
            }
            record(&self.directory, &format!("cloud.finished:{confirmed}"));
        })
    }
    fn aborted(&mut self) {
        record(&self.directory, "cloud.aborted");
    }
}

#[test]
fn force_quitting_ends_what_the_game_left_running_apart_from_it() {
    super::force_quitting_ends_what_the_game_left_running_apart_from_it::<Recording>();
}

/// A launch on the Linux engine of a runtime in `root`: how it runs, and the
/// runtime's folder.
fn on_the_linux_engine(root: &Path) -> (crate::wine::Config, PathBuf) {
    let release = root.join("Voile/v9.0.0-1");
    let runtime = release.join("linux");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(release.join("runtime.json"), r#"{"version":"v9.0.0-1"}"#).unwrap();
    let mut config = crate::wine::tests::config(root);
    config.settings.engine = crate::wine::Engine::Linux;
    config.runtime = runtime;
    (config, release)
}

fn held(release: &Path) -> bool {
    crate::voile::engine_patch::Replacing::hold(release)
        .unwrap()
        .is_none()
}

/// Another test's child may hold an inherited lock for the moment before its
/// exec.
fn free(release: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while held(release) {
        assert!(Instant::now() < deadline, "the runtime is still held");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Fifth verification, finding 2: the supervisor of a game on the Linux engine
/// holds its preparation's own lock from its first instant to its game's exit,
/// whatever becomes of the process that started it.
#[test]
fn a_supervisor_holds_its_preparation_s_pin_until_its_game_exits() {
    let wine = tempfile::tempdir().unwrap();
    let (config, release) = on_the_linux_engine(wine.path());
    let (_, pin) = crate::voile::engine_patch::apply(&config.runtime).unwrap();
    let (root, directory, sessions) = prepare(&Script {
        wine: Some(config),
        ..script(HELD)
    });
    let client = start_pinned::<Recording>(&directory, &sessions, Some(&pin)).unwrap();
    let run = Run::<Recording> {
        _root: root,
        directory,
        sessions,
        client,
    };
    // The process that prepared it lets go, as it does by crashing.
    drop(pin);
    run.until(|record| record.phase == Phase::Running);
    assert!(held(&release), "a running game's files are held");
    run.release("release-game");
    run.until(|record| record.finished());
    free(&release);
}

/// Finding 2: the lock is the child's from its fork on, so that a parent lost
/// before its supervisor did anything leaves no gap.
#[test]
fn a_pin_crosses_to_the_child_before_the_child_runs() {
    let wine = tempfile::tempdir().unwrap();
    let (config, release) = on_the_linux_engine(wine.path());
    let (_, pin) = crate::voile::engine_patch::apply(&config.runtime).unwrap();
    let mut child = Process::new("/bin/sleep")
        .arg("60")
        .stdout(pin.stdio().unwrap())
        .spawn()
        .unwrap();
    drop(pin);
    assert!(held(&release));
    std::thread::sleep(Duration::from_millis(100));
    assert!(held(&release));
    child.kill().unwrap();
    child.wait().unwrap();
    free(&release);
}

/// Finding 2: a launch on the Linux engine whose supervisor was given no pin,
/// or another runtime's, is never accepted: no record, no game.
#[test]
fn a_launch_on_the_linux_engine_without_its_pin_is_never_accepted() {
    let wine = tempfile::tempdir().unwrap();
    let (config, _) = on_the_linux_engine(wine.path());
    let other = tempfile::tempdir().unwrap();
    let (elsewhere, _) = on_the_linux_engine(other.path());
    let (_, foreign) = crate::voile::engine_patch::apply(&elsewhere.runtime).unwrap();
    for pin in [None, Some(&foreign)] {
        let (_root, directory, sessions) = prepare(&Script {
            wine: Some(config.clone()),
            ..script(QUICK)
        });
        assert!(start_pinned::<Recording>(&directory, &sessions, pin).is_err());
        assert!(!directory.join("ran").exists());
        assert!(
            crate::supervisor::journal::current::<Record<Recording>>(&sessions)
                .unwrap()
                .is_none()
        );
    }
}

/// A game on the Linux engine that writes as its guest does: into the pipe its
/// helper gets.
const WRITING: &str = r#"
exec 8>&2
echo "the guest begins" >&8
printf ran > ran
while [ ! -f release-game ]; do /bin/sleep 0.01; done
i=0; while [ $i -lt 20000 ]; do echo "line $i" >&8; i=$((i+1)); done
echo "the guest ends" >&8
printf done > wrote
exit 3
"#;

fn writing() -> (tempfile::TempDir, Run<Recording>) {
    let wine = tempfile::tempdir().unwrap();
    let (config, _) = on_the_linux_engine(wine.path());
    let (_, pin) = crate::voile::engine_patch::apply(&config.runtime).unwrap();
    let (root, directory, sessions) = prepare(&Script {
        wine: Some(config),
        ..script(WRITING)
    });
    // (the fixture's helper opens no log)
    fs::write(directory.join("game.log"), "").unwrap();
    let client = start_pinned::<Recording>(&directory, &sessions, Some(&pin)).unwrap();
    let run = Run::<Recording> {
        _root: root,
        directory,
        sessions,
        client,
    };
    (wine, run)
}

/// Fifth verification, finding 6: what a guest writes reaches its log through
/// its supervisor, the last of it before the session's record says it ended.
#[test]
fn what_a_guest_writes_reaches_its_log_through_its_supervisor() {
    let (_wine, run) = writing();
    run.until(|record| record.phase == Phase::Running);
    run.release("release-game");
    run.until(|record| record.finished());
    let log = fs::read_to_string(run.directory.join("game.log")).unwrap();
    assert!(log.starts_with("the guest begins\n"), "{log:.200}");
    assert!(log.contains("line 199\n") && log.contains("the guest ends\n"), "{log:.200}");
    assert!(log.contains("[obelisk-log: 20000 lines like this in all: line #]"), "{log:.200}");
    assert_eq!(run.client.snapshot(None).unwrap().exit_code, Some(3));
}

/// Finding 6: a game outlives the supervisor that reads what it writes.
#[test]
fn a_guest_outlives_its_supervisor() {
    let (_wine, run) = writing();
    let record = run.until(|record| record.phase == Phase::Running);
    run.wait_for("ran");
    assert!(
        Process::new("/bin/kill")
            .args(["-KILL", &record.supervisor.pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    std::thread::sleep(Duration::from_millis(200));
    run.release("release-game");
    // (more than a pipe holds: it is drained all the same)
    run.wait_for("wrote");
}

/// Sixth verification, finding 1 (the verifier's fixture): a supervisor killed
/// while its game runs lets nothing replace the game's files.
#[test]
fn a_killed_supervisor_s_game_keeps_its_files() {
    let (wine, run) = writing();
    let record = run.until(|record| record.phase == Phase::Running);
    run.wait_for("ran");
    let release = wine.path().join("Voile/v9.0.0-1");
    assert!(held(&release));
    assert!(
        Process::new("/bin/kill")
            .args(["-KILL", &record.supervisor.pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    for _ in 0..20 {
        assert!(held(&release), "its game still runs");
        std::thread::sleep(Duration::from_millis(25));
    }
    run.release("release-game");
    run.wait_for("wrote");
    free(&release);
}

/// Finding 1: a game whose supervisor is gone, and which no lock shows (an
/// older build's), is found by the record of its launch and by what it runs.
#[test]
fn a_game_no_lock_shows_keeps_its_files() {
    use crate::voile::live::tests::{Running, record};
    let library = tempfile::tempdir().unwrap();
    let (_, release) = on_the_linux_engine(library.path());
    free(&release);
    let program = Running::at(&release.join("linux/engine/fexdarwin-run"));
    assert!(held(&release), "what it runs");
    program.end();
    free(&release);
    // Rosetta's games hold no lock.
    let wine = Running::at(&release.join("bin/wine"));
    assert!(held(&release), "Rosetta's Wine");
    wine.end();
    free(&release);
    let elsewhere = Running::at(&library.path().join("launcher"));
    let game = elsewhere.identity();
    let gone = Identity { birth_seconds: 1, ..game.clone() };
    record(library.path(), "SteamSessions", gone, Some(game));
    assert!(held(&release), "its record");
    elsewhere.end();
    free(&release);
}

/// Sixth verification, the logging finding: a log that cannot be had starts
/// the game all the same, and the record says why it holds nothing.
#[test]
fn a_log_that_cannot_be_opened_refuses_no_game() {
    let wine = tempfile::tempdir().unwrap();
    let (config, _) = on_the_linux_engine(wine.path());
    let (_, pin) = crate::voile::engine_patch::apply(&config.runtime).unwrap();
    let (root, directory, sessions) = prepare(&Script {
        wine: Some(config),
        ..script(WRITING)
    });
    fs::create_dir(directory.join("game.log")).unwrap();
    let client = start_pinned::<Recording>(&directory, &sessions, Some(&pin)).unwrap();
    let run = Run::<Recording> {
        _root: root,
        directory,
        sessions,
        client,
    };
    let record = run.until(|record| record.log_failure.is_some());
    assert!(record.log_failure.unwrap().starts_with("the log cannot be opened"));
    run.release("release-game");
    run.wait_for("wrote");
    let record = run.until(|record| record.finished());
    assert_eq!((record.phase, record.exit_code), (Phase::Finished, Some(3)));
}
