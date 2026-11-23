//! Whether anything of a library's engine runs now: what is proven, under the
//! runtime's exclusive lock, before a file of any runtime of the library is
//! replaced, put back or removed. A lock alone does not prove it: a game
//! outlives a supervisor that died, Rosetta's games hold no pin, and the
//! builds before 0.1.9 held other locks or none.
//!
//! Two looks, and either refuses: the library's Game session records (their
//! supervisor, their game, the helper that becomes it), and every process
//! that runs a file under the library's runtimes (`fexdarwin-run`, the render
//! servers, Wine under Rosetta). The window server runs from a copy of its
//! own, with a launcher the records name. A look that cannot be made refuses
//! too.
use crate::supervisor::{Identity, StartingGame, journal, quit::listing};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};

/// Each storefront's records, beside the runtimes.
const SESSIONS: [&str; 3] = ["SteamSessions", "EpicSessions", "GOGSessions"];

/// What a Game session's record says of processes.
#[derive(Serialize, Deserialize)]
struct Recorded {
    id: String,
    supervisor: Identity,
    #[serde(default)]
    game: Option<Identity>,
    #[serde(default)]
    starting: Option<StartingGame>,
}

impl journal::Entry for Recorded {
    fn id(&self) -> &str {
        &self.id
    }
    fn valid(&self) -> bool {
        true
    }
}

/// Nothing of the engine of the library whose runtimes are in `root` runs.
/// `Err` tells what does, or why it cannot be told.
pub(super) fn idle(root: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("{} cannot be read ({error})", root.display()))?;
    sessions(&root)?;
    processes(&root)
}

fn sessions(root: &Path) -> Result<(), String> {
    let library = root.parent().ok_or("the runtimes are in no library")?;
    let mut recorded = Vec::new();
    for folder in SESSIONS {
        let record = journal::current::<Recorded>(&library.join(folder))
            .map_err(|error| format!("the Game session recorded in {folder} cannot be read ({error})"))?;
        let Some(record) = record else { continue };
        let starting = record.starting.map(|StartingGame::Process { identity, .. }| identity);
        recorded.extend([Some(record.supervisor), record.game, starting].into_iter().flatten());
    }
    if recorded.is_empty() {
        return Ok(());
    }
    let pids: Vec<_> = recorded.iter().map(|identity| identity.pid.to_string()).collect();
    let mut command = Command::new("/bin/ps");
    // (a list none of whose processes is there prints nothing)
    command.args(["-o", "pid=,uid=,lstart=", "-p", &pids.join(",")]);
    let told = listing(command).ok_or("the recorded processes cannot be listed")?;
    for line in told.lines().filter(|line| !line.trim().is_empty()) {
        let (pid, uid, born) = born(line).ok_or_else(|| format!("the process list reads {line:?}"))?;
        if recorded.iter().any(|known| (known.pid, known.uid, known.birth_seconds) == (pid, uid, born)) {
            return Err(format!("a Game session of this library runs (process {pid})"));
        }
    }
    Ok(())
}

/// The process, the user and the birth (seconds since 1970) of a line of
/// `ps -o pid=,uid=,lstart=` in UTC: `80745 501 Mon Nov 23 11:59:28 2026`.
fn born(line: &str) -> Option<(u32, u32, u64)> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [pid, uid, _weekday, month, day, clock, year] = fields[..] else {
        return None;
    };
    let month = MONTHS.iter().position(|name| *name == month)? as u64 + 1;
    let (day, year): (u64, u64) = (day.parse().ok()?, year.parse().ok()?);
    let mut clock = clock.split(':').map(|part| part.parse::<u64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    // Days since 1970-01-01 in the proleptic Gregorian calendar, a year beginning in March.
    let (shifted, from_march) = if month > 2 { (year, month - 3) } else { (year.checked_sub(1)?, month + 9) };
    let days = shifted * 365 + shifted / 4 - shifted / 100 + shifted / 400 + (from_march * 153 + 2) / 5 + day - 1;
    let days = days.checked_sub(719_468)?;
    Some((pid.parse().ok()?, uid.parse().ok()?, ((days * 24 + hour) * 60 + minute) * 60 + second))
}

fn processes(root: &Path) -> Result<(), String> {
    // lsof writes a name's control characters, and a backslash, in its own way.
    let plain = root.to_str().is_some_and(|path| !path.contains(|c: char| c.is_control() || c == '\\'));
    if !plain {
        return Err(format!("the processes running from {} cannot be told", root.display()));
    }
    let mut command = Command::new("/usr/sbin/lsof");
    command.args(["-n", "-P", "-w", "-d", "txt", "-Fpn"]);
    let own = std::process::id();
    // A listing that leaves this very process out lists nothing.
    let listed = listing(command)
        .filter(|listing| listing.lines().any(|line| line.strip_prefix('p') == Some(&own.to_string())))
        .ok_or("the running processes cannot be listed")?;
    match crate::wine::running_from(&listed, root, own).first() {
        Some(pid) => Err(format!("a program of this library's engine runs (process {pid})")),
        None => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
        process::{Child, Stdio},
    };

    /// A process running the program at `path`, put there: a copy of this
    /// test's own, which sleeps.
    pub(crate) struct Running(Child);

    impl Running {
        pub(crate) fn at(path: &Path) -> Self {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(std::env::current_exe().unwrap(), path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            let child = Command::new(path)
                .args(["--exact", "voile::live::tests::a_program_that_stays"])
                .env("OBELISK_TEST_STAYS", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Self(child)
        }

        pub(crate) fn identity(&self) -> Identity {
            let mut command = Command::new("/bin/ps");
            command.args(["-o", "pid=,uid=,lstart=", "-p", &self.0.id().to_string()]);
            let told = listing(command).unwrap();
            let (pid, uid, birth_seconds) = born(told.trim()).unwrap();
            Identity { pid, uid, birth_seconds, birth_microseconds: 1, executable: "/fixture/launcher".into() }
        }

        pub(crate) fn end(mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Not a test of its own: what `Running` runs.
    #[test]
    fn a_program_that_stays() {
        if std::env::var_os("OBELISK_TEST_STAYS").is_some() {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        }
    }

    fn library() -> (tempfile::TempDir, PathBuf) {
        let library = tempfile::tempdir().unwrap();
        let root = library.path().join("Voile");
        fs::create_dir_all(&root).unwrap();
        (library, root)
    }

    /// Records a session in `library`'s `folder`, as its supervisor does.
    pub(crate) fn record(library: &Path, folder: &str, supervisor: Identity, game: Option<Identity>) {
        let sessions = library.join(folder);
        fs::create_dir_all(&sessions).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let write = |name: String, value: serde_json::Value| {
            let path = sessions.join(name);
            fs::write(&path, value.to_string()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        };
        write("session.v2.json".into(), serde_json::json!({"version": 2, "id": id}));
        write(
            format!("{id}.v2.json"),
            serde_json::json!({"version": 2, "id": id, "supervisor": supervisor, "game": game, "phase": "Running"}),
        );
    }

    fn gone() -> Identity {
        Identity { pid: 1, uid: 0, birth_seconds: 1, birth_microseconds: 1, executable: "/gone".into() }
    }

    #[test]
    fn an_empty_library_is_idle() {
        let (_library, root) = library();
        idle(&root).unwrap();
        assert!(idle(&root.join("none")).is_err(), "a library that cannot be read is not");
    }

    #[test]
    fn a_birth_reads_as_the_epoch_tells_it() {
        let born = |line| born(line).unwrap().2;
        assert_eq!(born("1 0 Thu Jan  1 00:00:00 1970    "), 0);
        assert_eq!(born("1 0 Thu Feb 29 12:34:56 2024"), 1_709_210_096);
        assert_eq!(born("80745   501 Mon Nov 23 11:59:28 2026"), 1_795_435_168);
        assert!(super::born("1 0 lundi 23 nov. 11:59:28 2026").is_none());
        assert!(super::born("").is_none());
    }

    /// Sixth verification, finding 1: whatever runs from a runtime of the
    /// library is found by the kernel's word, with no record and no lock.
    #[test]
    fn a_program_of_the_engine_is_found_wherever_it_runs_from() {
        for (named, program) in [
            ("library", "v9.0.0-1/linux/engine/fexdarwin-run"),
            ("library", "v9.0.0-1/linux/graphics/bin/virgl_test_server"),
            ("library", "v9.0.0-1/bin/wine"),
            // a library under another alphabet, in spaces, through a link
            ("bibliothèque 図書", "v9.0.0-1/linux/engine/fexdarwin-run"),
        ] {
            let work = tempfile::tempdir().unwrap();
            let real = work.path().join(named);
            let root = real.join("Voile");
            fs::create_dir_all(&root).unwrap();
            let running = Running::at(&root.join(program));
            let link = work.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            for root in [&root, &link.join("Voile")] {
                assert!(idle(root).unwrap_err().contains("runs"), "{program}");
            }
            // Another library's is not this one's.
            let (_other, other) = library();
            idle(&other).unwrap();
            running.end();
            idle(&root).unwrap();
        }
    }

    /// Finding 1: a recorded game is found by its own birth, wherever it runs
    /// from, after its supervisor is gone; one gone, or another process under
    /// its number, is none.
    #[test]
    fn a_recorded_game_is_found_by_its_birth() {
        for folder in SESSIONS {
            let (library, root) = library();
            let elsewhere = tempfile::tempdir().unwrap();
            let running = Running::at(&elsewhere.path().join("game"));
            let game = running.identity();
            record(library.path(), folder, gone(), Some(game.clone()));
            assert!(idle(&root).unwrap_err().contains("Game session"), "{folder}");
            record(library.path(), folder, game.clone(), None);
            assert!(idle(&root).is_err(), "{folder}: its supervisor");
            let reused = Identity { birth_seconds: game.birth_seconds - 60, ..game.clone() };
            record(library.path(), folder, gone(), Some(reused));
            idle(&root).unwrap();
            record(library.path(), folder, gone(), Some(game));
            running.end();
            idle(&root).unwrap();
        }
    }

    /// Finding 1: what cannot be told refuses.
    #[test]
    fn a_record_that_cannot_be_read_is_never_taken_for_none() {
        let (library, root) = library();
        record(library.path(), SESSIONS[0], gone(), None);
        idle(&root).unwrap();
        let current = library.path().join(SESSIONS[0]).join("session.v2.json");
        for text in ["{ half", r#"{"version": 3, "id": "x"}"#] {
            fs::write(&current, text).unwrap();
            assert!(idle(&root).is_err(), "{text}");
        }
        fs::remove_file(&current).unwrap();
        idle(&root).unwrap();
        // The helper that becomes the game, before it has.
        let own = fs::metadata(library.path()).unwrap().uid();
        let id = uuid::Uuid::new_v4().to_string();
        let running = Running::at(&library.path().join("helper"));
        let starting = StartingGame::Process { identity: running.identity(), executable: "/game".into() };
        assert_eq!(running.identity().uid, own);
        for (name, value) in [
            ("session.v2.json".to_owned(), serde_json::json!({"version": 2, "id": id})),
            (format!("{id}.v2.json"), serde_json::json!({"version": 2, "id": id, "supervisor": gone(), "starting": starting})),
        ] {
            let path = library.path().join(SESSIONS[0]).join(name);
            fs::write(&path, value.to_string()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(idle(&root).unwrap_err().contains("Game session"));
    }
}
