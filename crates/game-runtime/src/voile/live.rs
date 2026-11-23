//! Whether anything of a library's engine runs now: what is proven, under the
//! library's exclusive lock, before a file of any runtime of the library is
//! replaced, put back or removed. A lock alone does not prove it: a game
//! outlives a supervisor that died, and the builds before 0.1.11 do not know
//! that lock.
//!
//! Two looks: the library's Game session records (their supervisor, their
//! game, the helper that becomes it), and every process that runs a file under
//! the library's runtimes (`fexdarwin-run`, the render servers, Wine under
//! Rosetta) or maps one, as `lsof` tells it. Both only read what the system
//! tells of processes: no signal, no task port, no attach. The window server
//! runs from a copy of its own, with a launcher the records name.
//!
//! The answer is one of three ([`Census`]): nothing runs, something does, or
//! it cannot be told, and only the first lets a file change. What cannot be
//! read whole is never taken for nothing: a command that failed, was killed,
//! took too long, said anything on its standard error or wrote what does not
//! read to its end; a session named whose record is missing or unreadable.
use crate::supervisor::{Identity, StartingGame, journal, quit};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

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

/// What the looks found.
#[derive(Debug, PartialEq, Eq)]
enum Census {
    /// Nothing of the library's engine runs.
    Clear,
    /// Something does: what.
    Alive(String),
    /// It cannot be told: why.
    Unknown(String),
}

/// The system's own listings, and how long one may take.
struct Tools {
    ps: PathBuf,
    lsof: PathBuf,
    timeout: Duration,
}

impl Tools {
    fn system() -> Self {
        Self {
            ps: "/bin/ps".into(),
            lsof: "/usr/sbin/lsof".into(),
            timeout: Duration::from_secs(10),
        }
    }
}

/// Nothing of the engine of the library whose runtimes are in `root` runs.
/// `Err` tells what does, or why it cannot be told.
pub(super) fn idle(root: &Path) -> Result<(), String> {
    match census(root, &Tools::system()) {
        Census::Clear => Ok(()),
        Census::Alive(what) | Census::Unknown(what) => Err(what),
    }
}

fn census(root: &Path, tools: &Tools) -> Census {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => return Census::Unknown(format!("{} cannot be read ({error})", root.display())),
    };
    match sessions(&root, tools) {
        Census::Clear => processes(&root, tools),
        found => found,
    }
}

fn sessions(root: &Path, tools: &Tools) -> Census {
    let Some(library) = root.parent() else {
        return Census::Unknown("the runtimes are in no library".into());
    };
    let mut recorded = Vec::new();
    for folder in SESSIONS {
        let record = match journal::named::<Recorded>(&library.join(folder)) {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(error) => {
                return Census::Unknown(format!("the Game session recorded in {folder} cannot be read ({error})"));
            }
        };
        let starting = record.starting.map(|StartingGame::Process { identity, .. }| identity);
        recorded.extend([Some(record.supervisor), record.game, starting].into_iter().flatten());
    }
    if recorded.is_empty() {
        return Census::Clear;
    }
    let pids: Vec<_> = recorded.iter().map(|identity| identity.pid.to_string()).collect();
    let mut command = Command::new(&tools.ps);
    command.args(["-o", "pid=,uid=,lstart=", "-p", &pids.join(",")]);
    let told = match quit::answer(command, tools.timeout) {
        quit::Answer::Listed(told) => told,
        // None of them is there.
        quit::Answer::Nothing => return Census::Clear,
        quit::Answer::Failed(why) => {
            return Census::Unknown(format!("the recorded processes cannot be listed: {why}"));
        }
    };
    if !told.ends_with('\n') {
        return Census::Unknown("the list of the recorded processes is not whole".into());
    }
    for line in told.lines() {
        let Some((pid, uid, born)) = born(line) else {
            return Census::Unknown(format!("the process list reads {line:?}"));
        };
        if recorded.iter().any(|known| (known.pid, known.uid, known.birth_seconds) == (pid, uid, born)) {
            return Census::Alive(format!("a Game session of this library runs (process {pid})"));
        }
    }
    Census::Clear
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

fn processes(root: &Path, tools: &Tools) -> Census {
    // lsof writes a name's control characters, and a backslash, in its own way.
    let plain = root.to_str().is_some_and(|path| !path.contains(|c: char| c.is_control() || c == '\\'));
    if !plain {
        return Census::Unknown(format!("the processes running from {} cannot be told", root.display()));
    }
    let mut command = Command::new(&tools.lsof);
    command.args(["-n", "-P", "-w", "-d", "txt", "-Fpn"]);
    let listed = match quit::answer(command, tools.timeout) {
        quit::Answer::Listed(listed) => listed,
        // This very process is there to be listed.
        quit::Answer::Nothing => return Census::Unknown("the running processes cannot be listed: none was".into()),
        quit::Answer::Failed(why) => {
            return Census::Unknown(format!("the running processes cannot be listed: {why}"));
        }
    };
    let Some(files) = mapped(&listed) else {
        return Census::Unknown("the list of the running processes is not whole".into());
    };
    let own = std::process::id();
    if !files.iter().any(|(pid, _)| *pid == own) {
        return Census::Unknown("the list of the running processes leaves this one out".into());
    }
    match files.iter().find(|(pid, path)| *pid != own && Path::new(path).starts_with(root)) {
        Some((pid, path)) => Census::Alive(format!("process {pid} runs {path}")),
        None => Census::Clear,
    }
}

/// The processes of an `lsof -Fpn` listing with each file they run or map;
/// `None` for one that is not whole, or not such a listing: a process is
/// followed by a file or more, each file by its name.
fn mapped(listing: &str) -> Option<Vec<(u32, &str)>> {
    if !listing.ends_with('\n') {
        return None;
    }
    let mut pid = None;
    let mut files = Vec::new();
    let mut before = b'n';
    for line in listing.lines() {
        let field = *line.as_bytes().first()?;
        match (before, field) {
            (b'n', b'p') => pid = Some(line[1..].parse().ok()?),
            (b'p' | b'n', b'f') if line == "ftxt" => {}
            (b'f', b'n') if line.len() > 1 => files.push((pid?, &line[1..])),
            _ => return None,
        }
        before = field;
    }
    (before == b'n').then_some(files)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
        process::{Child, Stdio},
        time::Instant,
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
            let told = quit::listing(command).unwrap();
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

    /// A command that answers as `script` does, in place of the system's.
    fn fake(folder: &Path, name: &str, script: &str) -> PathBuf {
        let path = folder.join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Seventh verification, blocker 1 (the verifier's listing fixture): a
    /// listing that failed, or is not whole, is never "nothing runs"; one that
    /// found nothing, as the command says it, is.
    #[test]
    fn a_listing_that_failed_is_not_nothing() {
        let (library, root) = library();
        let fakes = tempfile::tempdir().unwrap();
        record(library.path(), SESSIONS[0], gone(), None);
        let own = std::process::id();
        let whole = format!("printf 'p{own}\\nftxt\\nn/bin/sh\\np1\\nftxt\\nn/sbin/launchd\\n'");
        let mut tools = Tools::system();
        let mut look = |ps: Option<&str>, lsof: Option<&str>| {
            tools.ps = ps.map_or("/bin/ps".into(), |script| fake(fakes.path(), "ps", script));
            tools.lsof = lsof.map_or("/usr/sbin/lsof".into(), |script| fake(fakes.path(), "lsof", script));
            census(&root, &tools)
        };
        assert_eq!(look(None, None), Census::Clear, "the system's own");
        // `ps` found none of the processes: status 1, not a word.
        assert_eq!(look(Some("exit 1"), Some(&whole)), Census::Clear);
        for failed in [
            "exit 17",
            "printf 'partial fixture output'; exit 17",
            "printf '  1     0 Thu Jan  1 00:00:00 1970\\n'; exit 1",
            "echo 'ps: no such thing' >&2; exit 1",
            "echo 'ps: something is wrong' >&2; exit 0",
            "printf '  1     0 Thu Jan  1 00:00'",
            "printf '  1     0 Thu Jan  1 00:00:00 1970'",
            "printf 'what is this\\n'",
            "kill -KILL $$",
            "kill -TERM $$",
        ] {
            assert!(matches!(look(Some(failed), Some(&whole)), Census::Unknown(_)), "ps: {failed}");
        }
        for failed in [
            // "nothing found" leaves out this very process: no listing at all.
            "exit 1".to_owned(),
            "exit 0".to_owned(),
            "exit 17".to_owned(),
            // The verifier's: one that names this process, and failed.
            format!("{whole}; exit 17"),
            format!("{whole}; exit 1"),
            format!("{whole}; echo 'lsof: cannot read the rest' >&2"),
            format!("{whole}; kill -KILL $$"),
            format!("{whole}; printf 'p2\\nftxt\\nn/Vol'"),
            format!("{whole}; printf 'p2x\\n'"),
            format!("{whole}; printf 'what is this\\n'"),
            format!("printf 'n/no/process\\n'; {whole}"),
            "printf 'p1\\nftxt\\nn/sbin/launchd\\n'".to_owned(),
            // Eighth verification, minor (the verifier's): a last record cut at the end of a line.
            format!("{whole}; printf 'p2\\n'"),
            format!("{whole}; printf 'p2\\nftxt\\n'"),
            format!("{whole}; printf 'p2\\nftxt\\nn\\n'"),
            format!("{whole}; printf 'p2\\nftxt\\nn/bin/sh\\nftxt\\n'"),
        ] {
            assert!(matches!(look(None, Some(&failed)), Census::Unknown(_)), "lsof: {failed}");
        }
        assert_eq!(look(None, Some(&whole)), Census::Clear);
        let runs = format!("{whole}; printf 'p77\\nftxt\\nn{}/v9.0.0-1/linux/engine/fexdarwin-run\\n'", root.canonicalize().unwrap().display());
        match look(None, Some(&runs)) {
            Census::Alive(what) => assert!(what.contains("77") && what.contains("fexdarwin-run"), "{what}"),
            found => panic!("{found:?}"),
        }
        // Neither can be run; neither ends.
        for (ps, lsof) in [("/bin/ps", "/missing"), ("/missing", "/usr/sbin/lsof")] {
            let tools = Tools { ps: ps.into(), lsof: lsof.into(), ..Tools::system() };
            assert!(matches!(census(&root, &tools), Census::Unknown(_)));
        }
        let slow = fake(fakes.path(), "slow", "exec /bin/sleep 30");
        let begun = Instant::now();
        let tools = Tools { lsof: slow, timeout: Duration::from_millis(300), ..Tools::system() };
        assert!(matches!(census(&root, &tools), Census::Unknown(_)));
        assert!(begun.elapsed() < Duration::from_secs(10));
    }

    /// Blocker 1: a session that is named and whose record is gone is not no
    /// session.
    #[test]
    fn a_named_session_without_its_record_is_not_none() {
        for folder in SESSIONS {
            let (library, root) = library();
            record(library.path(), folder, gone(), None);
            idle(&root).unwrap();
            let sessions = library.path().join(folder);
            for entry in fs::read_dir(&sessions).unwrap().flatten() {
                if entry.file_name() != "session.v2.json" {
                    fs::remove_file(entry.path()).unwrap();
                }
            }
            assert!(idle(&root).unwrap_err().contains(folder), "{folder}");
        }
    }
}
