//! The Orbiters fork's engine files. Voile's Linux engine takes every Mac
//! keyboard for a US one: its window server names a key by its place, and its
//! Wine driver has no layout. And Neptune's render server refused any one
//! resource update over 64 MiB, which VRChat sends seconds after an avatar
//! loads: the game's graphics went for good. Four files of ours, built from
//! Voile's own sources plus our changes (`scripts/build-engine-patch.sh`,
//! `engine-patch/manifest.json`), replace the release's own before a game
//! starts on the engine: the window server then tells the layout selected on
//! the Mac, the driver gives the game its virtual keys and characters, and
//! the render servers (arm64 for DXMT, x86_64 for D3DMetal) take an update up
//! to 1 GiB.
//!
//! They are built for one release and go over that release only, and only
//! over the files it shipped, told by their SHA-256: any other release, a
//! development runtime, or a file that is neither Voile's nor ours is left
//! as it is, and its games run as Voile ships them. The files go in as one:
//! under a lock of the runtime, every file is written and checked beside the
//! one it replaces first, each original is kept aside, then all are switched;
//! a switch that fails part way puts the originals back, and one a crash cut
//! short is undone at the next launch before anything else. A runtime is
//! never left with some of our files and some of Voile's by this program.
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, sync::OnceLock};

const MANIFEST: &str = include_str!("../../engine-patch/manifest.json");
/// The files built in, by the manifest's `source` names.
const FILES: [(&str, &[u8]); 5] = [
    ("wsd", include_bytes!("../../engine-patch/wsd")),
    ("wineobelisk.so", include_bytes!("../../engine-patch/wineobelisk.so")),
    ("virgl_render_server", include_bytes!("../../engine-patch/virgl_render_server")),
    ("virgl_render_server-x86_64", include_bytes!("../../engine-patch/virgl_render_server-x86_64")),
    ("fexdarwin-run", include_bytes!("../../engine-patch/fexdarwin-run")),
];

#[derive(Deserialize)]
struct Manifest {
    /// The one Voile release the files are built for.
    voile: String,
    files: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    /// In the Linux engine's runtime.
    path: String,
    /// The file built in, by its name in `engine-patch/`.
    source: String,
    /// The SHA-256 of the file the release ships.
    original: String,
    /// The SHA-256 of ours.
    patched: String,
    /// The SHA-256 of every file an earlier obelisk-cli put at this path: ours too, replaced like the
    /// release's own (`scripts/build-engine-patch.sh` lists them from the manifest's history).
    #[serde(default)]
    previous: Vec<String>,
}

/// Replacement files for the Linux engine of one Voile release.
pub(crate) struct Patch {
    release: String,
    files: Vec<File>,
}

struct File {
    path: String,
    original: String,
    patched: String,
    previous: Vec<String>,
    bytes: &'static [u8],
}

fn built_in() -> &'static Patch {
    static VALUE: OnceLock<Patch> = OnceLock::new();
    VALUE.get_or_init(|| {
        let manifest: Manifest =
            serde_json::from_str(MANIFEST).expect("checked-in engine patch manifest");
        Patch {
            release: manifest.voile,
            files: manifest
                .files
                .into_iter()
                .map(|entry| File {
                    bytes: FILES
                        .iter()
                        .find(|(source, _)| *source == entry.source)
                        .map(|(_, bytes)| *bytes)
                        .expect("every file the manifest names is built in"),
                    path: entry.path,
                    original: entry.original,
                    patched: entry.patched,
                    previous: entry.previous,
                })
                .collect(),
        }
    })
}

/// Which keyboard the games of a Linux engine runtime get, and with it
/// whether its render servers take updates over 64 MiB: one patch does both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// Our files are in place: the layout selected on the Mac, updates up to
    /// 1 GiB.
    Mac,
    /// The release is the one our files are for and still has some of its
    /// own: the next launch on the engine puts ours in place.
    Pending,
    /// Another release, or files that are neither the release's nor ours: a
    /// US keyboard, whatever the Mac's layout, and Voile's render servers.
    Us,
}

impl Keyboard {
    /// As `obelisk-cli status` tells it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mac => "mac",
            Self::Pending => "pending",
            Self::Us => "us",
        }
    }
}

/// What a file of the runtime is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Found {
    Original,
    /// Ours, from an earlier obelisk-cli: replaced as the release's own is.
    Previous,
    Patched,
    /// A file neither the release's nor ours (or none at that path): the runtime is left as it is.
    Other,
    /// The file is there but cannot be read: never taken for "foreign", the launch is refused.
    Unreadable(String),
}

/// The engine files are in a state no game may start on, and this program cannot repair it alone: the
/// paths, what is wrong and what the player (or the app) can do. Nothing was changed by the refusal.
#[derive(Debug, PartialEq, Eq)]
pub struct Incoherent {
    /// The paths, in the runtime, that are not what they must be.
    pub paths: Vec<String>,
    /// What is wrong, and what to do.
    pub reason: String,
}

impl std::fmt::Display for Incoherent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.reason, self.paths.join(", "))
    }
}

/// A runtime generation held: while any process holds it no engine file of the runtime changes (an update
/// needs every holder gone). The preparation holds one until the session's supervisor holds its own, which
/// lasts until the game has exited (`docs/CLI.md`, "The engine files").
#[derive(Debug)]
pub struct Pin {
    _file: Option<fs::File>,
}

impl Pin {
    /// A pin of `linux`: blocks until no update is in progress (a few seconds at most).
    pub fn hold(linux: &Path) -> std::io::Result<Self> {
        let file = open_lock(&linux.join(SESSIONS_LOCK))?;
        file.lock_shared()?;
        Ok(Self { _file: Some(file) })
    }
    fn none() -> Self {
        Self { _file: None }
    }
}

const PATCH_LOCK: &str = ".obelisk-engine-patch.lock";
const SESSIONS_LOCK: &str = ".obelisk-engine-sessions.lock";
const JOURNAL: &str = ".obelisk-engine-patch.txn.json";

/// The transaction record, written (and synced) before each phase. `before`/`after` are the exact SHA-256 of
/// every file of the set before and after: a crash at any point is undone or completed from it alone.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Journal {
    id: String,
    phase: Phase,
    /// The Voile release the transaction was for: checked before anything is recovered.
    release: String,
    files: Vec<JournalFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Phase {
    /// Our files being written beside the destinations, originals linked aside: nothing switched.
    Staging,
    /// Being switched: any file may be before or after.
    Switching,
    /// All switched and checked: only the kept originals are left to remove.
    Committed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct JournalFile {
    path: String,
    before: String,
    after: String,
}

/// Test injection points of a transaction (`Patch::prepare_with`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Faults {
    /// Writing the journal fails (a full disk) at its n-th write (0: the first).
    journal_write: Option<usize>,
    /// The process dies (returns at once, nothing cleaned) right after phase `crash_after` is recorded, or,
    /// with `crash_switched`, after that many files were switched.
    crash_after: Option<Phase>,
    crash_switched: Option<usize>,
    /// Staging file n fails (a full disk).
    stage: Option<usize>,
    /// Switching file n fails.
    switch: Option<usize>,
    /// Putting back a switched file fails too.
    undo: bool,
    /// Removing the kept originals after the commit fails.
    remove_backups: bool,
}

struct Crashed;

impl File {
    fn found_sha(&self, sha: &str) -> Found {
        if sha == self.patched {
            Found::Patched
        } else if sha == self.original {
            Found::Original
        } else if self.previous.iter().any(|known| known == sha) {
            Found::Previous
        } else {
            Found::Other
        }
    }
}

fn read_sha(path: &Path) -> Result<Option<String>, String> {
    match crate::sha(path) {
        Ok(sha) => Ok(Some(sha)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn open_lock(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)
}

fn sync_dir(path: &Path) {
    if let Some(dir) = path.parent().and_then(|folder| fs::File::open(folder).ok()) {
        let _ = dir.sync_all();
    }
}

/// The names a transaction uses beside a destination: tied to the transaction and to that destination.
fn txn_name(path: &Path, id: &str, what: &str) -> std::path::PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".obelisk-txn-{id}-{what}-{name}"))
}

/// Where an obelisk-cli before 0.1.8 kept the original of `path` during a switch (no transaction record).
fn aside_of(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".obelisk-original-{name}"))
}

impl Patch {
    fn found(&self, linux: &Path) -> Vec<Found> {
        self.files
            .iter()
            .map(|file| match read_sha(&linux.join(&file.path)) {
                Ok(Some(sha)) => file.found_sha(&sha),
                Ok(None) => Found::Other,
                Err(error) => Found::Unreadable(error),
            })
            .collect()
    }

    fn keyboard_of(&self, linux: &Path, found: &[Found]) -> Keyboard {
        if crate::voile::release_of(linux).as_deref() != Some(self.release.as_str()) {
            return Keyboard::Us;
        }
        if found.iter().any(|found| matches!(found, Found::Other | Found::Unreadable(_))) {
            Keyboard::Us
        } else if found.iter().all(|found| *found == Found::Patched) {
            Keyboard::Mac
        } else {
            Keyboard::Pending
        }
    }

    fn keyboard(&self, linux: &Path) -> Keyboard {
        self.keyboard_of(linux, &self.found(linux))
    }

    /// What each file of the runtime is, as `status` tells it: `voile`, `ours`, `ours-earlier`, `other` or
    /// `unreadable`, by its path in the runtime.
    fn components(&self, linux: &Path) -> Vec<(String, &'static str)> {
        self.files
            .iter()
            .zip(self.found(linux))
            .map(|(file, found)| {
                let state = match found {
                    Found::Original => "voile",
                    Found::Previous => "ours-earlier",
                    Found::Patched => "ours",
                    Found::Other => "other",
                    Found::Unreadable(_) => "unreadable",
                };
                (file.path.clone(), state)
            })
            .collect()
    }

    fn refuse(&self, linux: &Path, paths: Vec<String>, problem: &str) -> Incoherent {
        let action = format!(
            "No game starts on these engine files until this is resolved: reinstall Voile's release {} (or remove \
             {} and the .obelisk-txn-* files beside the files named, then start again so that the files are \
             checked anew)",
            self.release,
            linux.join(JOURNAL).display()
        );
        Incoherent { paths, reason: format!("{problem}. {action}") }
    }

    /// Our files over the release's own (or an earlier obelisk-cli's) in `linux`, all of them or none, when
    /// every one is the release's or ours and no session is running on the runtime; returns the keyboard
    /// the launch gets and a pin that keeps the runtime's files as they are until it is dropped. `Err` when
    /// the files are not one complete set this program can start on or repair (nothing is changed then).
    fn prepare(&self, linux: &Path) -> Result<(Keyboard, Pin), Incoherent> {
        match self.prepare_with(linux, Faults::default()) {
            Ok(done) => done,
            Err(Crashed) => unreachable!("no fault was injected"),
        }
    }

    fn prepare_with(&self, linux: &Path, faults: Faults) -> Result<Result<(Keyboard, Pin), Incoherent>, Crashed> {
        // Writers are serialised by the patch lock; sessions hold the sessions lock shared.
        let patch_lock = open_lock(&linux.join(PATCH_LOCK)).and_then(|file| file.lock().map(|()| file));
        let Ok(_patch_lock) = patch_lock else {
            // No lock (a runtime this user cannot write): nobody can change the files either. Start on them
            // only if they are one complete set and no transaction is pending.
            if linux.join(JOURNAL).exists() {
                return Ok(Err(self.refuse(
                    linux,
                    vec![JOURNAL.to_owned()],
                    "an engine update was cut short and the runtime's lock cannot be taken to finish it",
                )));
            }
            return Ok(self.verified(linux).map(|keyboard| (keyboard, Pin::none())));
        };
        let sessions = match open_lock(&linux.join(SESSIONS_LOCK)) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("engine patch: {}: {error}: the engine files are not updated", linux.join(SESSIONS_LOCK).display());
                if linux.join(JOURNAL).exists() {
                    return Ok(Err(self.refuse(linux, vec![JOURNAL.to_owned()], "an engine update was cut short")));
                }
                return Ok(self.verified(linux).map(|keyboard| (keyboard, Pin::none())));
            }
        };
        let exclusive = match sessions.try_lock() {
            Ok(()) => true,
            Err(fs::TryLockError::WouldBlock) => false,
            Err(fs::TryLockError::Error(_)) => false,
        };
        let done = self.under_locks(linux, exclusive, faults)?;
        // Hand over from the writer's lock to the session's: no update can come between (we still hold the
        // patch lock, which every writer needs).
        if exclusive {
            let _ = sessions.unlock();
        }
        Ok(match done {
            Ok(keyboard) => match sessions.lock_shared() {
                Ok(()) => Ok((keyboard, Pin { _file: Some(sessions) })),
                Err(error) => Err(self.refuse(
                    linux,
                    vec![SESSIONS_LOCK.to_owned()],
                    &format!("the runtime's session lock cannot be held ({error})"),
                )),
            },
            Err(incoherent) => Err(incoherent),
        })
    }

    /// The files are one complete set a game may start on, as they are.
    fn verified(&self, linux: &Path) -> Result<Keyboard, Incoherent> {
        let found = self.found(linux);
        let unreadable: Vec<String> = self
            .files
            .iter()
            .zip(&found)
            .filter(|(_, found)| matches!(found, Found::Unreadable(_)))
            .map(|(file, _)| file.path.clone())
            .collect();
        if !unreadable.is_empty() {
            let why = found.iter().find_map(|f| if let Found::Unreadable(e) = f { Some(e.clone()) } else { None });
            return Err(self.refuse(
                linux,
                unreadable,
                &format!("an engine file cannot be read ({})", why.unwrap_or_default()),
            ));
        }
        Ok(self.keyboard_of(linux, &found))
    }

    fn under_locks(&self, linux: &Path, exclusive: bool, faults: Faults) -> Result<Result<Keyboard, Incoherent>, Crashed> {
        // 1. The release first: nothing of another release is recovered or installed.
        let release = crate::voile::release_of(linux);
        let journal_path = linux.join(JOURNAL);
        if journal_path.exists() {
            let journal = match fs::read(&journal_path).map_err(|e| e.to_string()).and_then(|bytes| {
                serde_json::from_slice::<Journal>(&bytes).map_err(|e| e.to_string())
            }) {
                Ok(journal) => journal,
                Err(error) => {
                    return Ok(Err(self.refuse(
                        linux,
                        vec![JOURNAL.to_owned()],
                        &format!("the record of an engine update cannot be read ({error})"),
                    )))
                }
            };
            if release.as_deref() != Some(journal.release.as_str()) {
                return Ok(Err(self.refuse(
                    linux,
                    vec![JOURNAL.to_owned()],
                    &format!(
                        "an engine update for Voile {} was cut short in a runtime that is now {}",
                        journal.release,
                        release.as_deref().unwrap_or("of no known release")
                    ),
                )));
            }
            if !exclusive {
                return Ok(Err(self.refuse(
                    linux,
                    vec![JOURNAL.to_owned()],
                    "an engine update was cut short while another game runs on this runtime: quit it, then start again",
                )));
            }
            if let Err(incoherent) = self.recover(linux, &journal) {
                return Ok(Err(incoherent));
            }
        }
        if release.as_deref() != Some(self.release.as_str()) {
            return Ok(Ok(Keyboard::Us)); // another release: as Voile ships it, untouched
        }
        if exclusive {
            self.clean_leftovers(linux);
        }
        let found = match self.verified(linux) {
            Ok(_) => self.found(linux),
            Err(incoherent) => return Ok(Err(incoherent)),
        };
        let keyboard = self.keyboard_of(linux, &found);
        if keyboard != Keyboard::Pending || !exclusive {
            // all ours, someone else's files, or a game running on this set: start on it as it is
            return Ok(Ok(keyboard));
        }
        self.transaction(linux, &found, faults)
    }

    /// Files an obelisk-cli before 0.1.8 kept aside (no record: they are never installed while the file at
    /// the destination is one of the set; only the sole copy of a file that is gone is put back), transaction
    /// files of no pending transaction, and old temporaries.
    fn clean_leftovers(&self, linux: &Path) {
        let mut folders = std::collections::BTreeSet::new();
        for file in &self.files {
            let path = linux.join(&file.path);
            let aside = aside_of(&path);
            if aside.is_file() {
                let current = read_sha(&path);
                let kept = read_sha(&aside);
                let known = |sha: &Option<String>| {
                    sha.as_deref().is_some_and(|sha| matches!(file.found_sha(sha), Found::Original | Found::Previous | Found::Patched))
                };
                match (current, kept) {
                    (Ok(None), Ok(kept)) if known(&kept) => {
                        // the only copy of this file: it goes back (a hard link, the kept name then goes)
                        if fs::hard_link(&aside, &path).is_ok() {
                            let _ = fs::remove_file(&aside);
                        }
                    }
                    (Ok(Some(current)), _) if known(&Some(current.clone())) => {
                        // the file at the destination is one of the set: the kept copy is never used
                        if fs::remove_file(&aside).is_err() {
                            eprintln!("engine patch: {} cannot be removed; it is not used", aside.display());
                        }
                    }
                    _ => eprintln!("engine patch: {} is kept as it is (not one of our files there)", aside.display()),
                }
            }
            if let Some(folder) = path.parent() {
                folders.insert(folder.to_owned());
            }
        }
        for folder in folders {
            for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let legacy_temp = name.starts_with(".obelisk-")
                    && !name.starts_with(".obelisk-original-")
                    && !name.starts_with(".obelisk-engine-");
                if legacy_temp {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    fn write_journal(&self, linux: &Path, journal: &Journal, writes: &mut usize, faults: Faults) -> std::io::Result<()> {
        let index = *writes;
        *writes += 1;
        if faults.journal_write == Some(index) {
            return Err(std::io::Error::other("a test's full disk"));
        }
        let path = linux.join(JOURNAL);
        let temp = linux.join(format!("{JOURNAL}.new"));
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(journal).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        sync_dir(&path);
        Ok(())
    }

    fn remove_journal(linux: &Path) -> std::io::Result<()> {
        let path = linux.join(JOURNAL);
        let result = fs::remove_file(&path);
        sync_dir(&path);
        match result {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn transaction(&self, linux: &Path, found: &[Found], faults: Faults) -> Result<Result<Keyboard, Incoherent>, Crashed> {
        let id = format!(
            "{:x}{:x}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
            std::process::id()
        );
        let mut journal = Journal {
            id: id.clone(),
            phase: Phase::Staging,
            release: self.release.clone(),
            files: Vec::new(),
        };
        for file in &self.files {
            let before = read_sha(&linux.join(&file.path)).ok().flatten().unwrap_or_default();
            journal.files.push(JournalFile { path: file.path.clone(), before, after: file.patched.clone() });
        }
        let mut writes = 0;
        let pending = Ok(Keyboard::Pending);
        // A journal that cannot be written (a full disk): nothing changes, the launch runs on the set as it is.
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            let _ = Self::remove_journal(linux);
            return Ok(pending);
        }
        if faults.crash_after == Some(Phase::Staging) {
            return Err(Crashed);
        }
        let switch: Vec<&File> = self.files.iter().zip(found).filter(|(_, f)| **f != Found::Patched).map(|(f, _)| f).collect();
        let cleanup = |linux: &Path| {
            for file in &self.files {
                let path = linux.join(&file.path);
                let _ = fs::remove_file(txn_name(&path, &id, "new"));
                let _ = fs::remove_file(txn_name(&path, &id, "old"));
            }
        };
        for (index, file) in switch.iter().enumerate() {
            let path = linux.join(&file.path);
            let staged = if faults.stage == Some(index) {
                Err(std::io::Error::other("a test's full disk"))
            } else {
                stage(&path, &txn_name(&path, &id, "new"), file.bytes, &file.patched)
            };
            let kept = staged.and_then(|()| fs::hard_link(&path, txn_name(&path, &id, "old")));
            if kept.is_err() {
                cleanup(linux);
                let _ = Self::remove_journal(linux);
                return Ok(pending);
            }
        }
        journal.phase = Phase::Switching;
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            cleanup(linux);
            let _ = Self::remove_journal(linux);
            return Ok(pending);
        }
        if faults.crash_after == Some(Phase::Switching) && faults.crash_switched.is_none() {
            return Err(Crashed);
        }
        for (index, file) in switch.iter().enumerate() {
            if faults.crash_switched == Some(index) {
                return Err(Crashed);
            }
            let path = linux.join(&file.path);
            let done = if faults.switch == Some(index) {
                Err(std::io::Error::other("a test's failure"))
            } else {
                fs::rename(txn_name(&path, &id, "new"), &path)
            };
            if done.is_err() {
                return Ok(self.roll_back_now(linux, &journal, faults.undo));
            }
        }
        for file in &switch {
            sync_dir(&linux.join(&file.path));
        }
        // Every file checked against the record before the commit is.
        if self.files.iter().any(|file| read_sha(&linux.join(&file.path)).ok().flatten().as_deref() != Some(file.patched.as_str())) {
            return Ok(self.roll_back_now(linux, &journal, faults.undo));
        }
        journal.phase = Phase::Committed;
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            // the record on disk still says Switching: undo as from it
            journal.phase = Phase::Switching;
            return Ok(self.roll_back_now(linux, &journal, faults.undo));
        }
        if faults.crash_after == Some(Phase::Committed) {
            return Err(Crashed);
        }
        // Committed: what is left is removing the kept originals, which may fail without consequence (the
        // record stays; the next launch only removes them, never puts any back).
        let mut removed = !faults.remove_backups;
        if removed {
            for file in &switch {
                let path = linux.join(&file.path);
                removed &= fs::remove_file(txn_name(&path, &id, "old")).is_ok();
            }
        }
        if removed {
            let _ = Self::remove_journal(linux);
        }
        Ok(Ok(self.keyboard(linux)))
    }

    /// A switch failed: every file back to its `before`, checked; the record goes when that worked, and stays
    /// (for the next launch, which retries) when it did not.
    fn roll_back_now(&self, linux: &Path, journal: &Journal, fail: bool) -> Result<Keyboard, Incoherent> {
        if fail {
            return Err(self.refuse(
                linux,
                journal.files.iter().map(|f| f.path.clone()).collect(),
                "an engine update failed part way and could not be undone (a test's failure)",
            ));
        }
        self.recover(linux, journal).map(|()| Keyboard::Pending)
    }

    /// Ends a transaction a crash (or a failure) left: `Committed` only removes its kept originals; any other
    /// phase puts every file back to its `before` from the original kept for this transaction and this
    /// destination, checked by digest. A file or a kept original that is unreadable, or not the digest the
    /// record names, is never guessed at: refused, naming it.
    fn recover(&self, linux: &Path, journal: &Journal) -> Result<(), Incoherent> {
        let mut problems = Vec::new();
        let mut why = String::new();
        for record in &journal.files {
            let path = linux.join(&record.path);
            let old = txn_name(&path, &journal.id, "old");
            let current = match read_sha(&path) {
                Ok(sha) => sha,
                Err(error) => {
                    problems.push(record.path.clone());
                    why = format!("{} cannot be read ({error})", path.display());
                    continue;
                }
            };
            if journal.phase == Phase::Committed {
                if current.as_deref() != Some(record.after.as_str()) {
                    problems.push(record.path.clone());
                    why = format!("{} changed after an engine update was committed", path.display());
                }
                continue;
            }
            if current.as_deref() == Some(record.before.as_str()) || (record.before.is_empty() && current.is_none()) {
                continue; // not switched
            }
            if current.as_deref() != Some(record.after.as_str()) {
                problems.push(record.path.clone());
                why = format!("{} is neither the file before the update nor ours", path.display());
                continue;
            }
            match read_sha(&old) {
                Ok(Some(sha)) if sha == record.before => {
                    if let Err(error) = fs::rename(&old, &path) {
                        problems.push(record.path.clone());
                        why = format!("{} cannot be put back from {} ({error})", path.display(), old.display());
                    } else {
                        sync_dir(&path);
                    }
                }
                Ok(Some(_)) => {
                    problems.push(record.path.clone());
                    why = format!("{} is not the file this update replaced", old.display());
                }
                Ok(None) => {
                    problems.push(record.path.clone());
                    why = format!("the original of {} kept for this update ({}) is missing", path.display(), old.display());
                }
                Err(error) => {
                    problems.push(record.path.clone());
                    why = format!("the original of {} kept for this update ({}) cannot be read ({error})", path.display(), old.display());
                }
            }
        }
        if !problems.is_empty() {
            return Err(self.refuse(linux, problems, &format!("an engine update cut short cannot be ended: {why}")));
        }
        // Every file is its `before` (or, committed, its `after`): the transaction's own files go, then the record.
        for record in &journal.files {
            let path = linux.join(&record.path);
            for what in ["old", "new"] {
                let file = txn_name(&path, &journal.id, what);
                if file.exists() && fs::remove_file(&file).is_err() && journal.phase != Phase::Committed {
                    eprintln!("engine patch: {} cannot be removed", file.display());
                }
            }
        }
        if Self::remove_journal(linux).is_err() && journal.phase != Phase::Committed {
            return Err(self.refuse(linux, vec![JOURNAL.to_owned()], "the record of an ended engine update cannot be removed"));
        }
        Ok(())
    }
}

/// Our file written at `to` (beside `path`) with `path`'s mode, synced, and checked against `sha`.
fn stage(path: &Path, to: &Path, bytes: &[u8], sha: &str) -> std::io::Result<()> {
    let mode = fs::metadata(path)?.permissions().mode();
    let _ = fs::remove_file(to);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(to)?;
    file.write_all(bytes)?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    if crate::sha(to).ok().as_deref() != Some(sha) {
        return Err(std::io::Error::other("the staged file is not ours"));
    }
    Ok(())
}

/// Which keyboard the games of the Linux engine runtime `linux` get.
pub fn keyboard(linux: &Path) -> Keyboard {
    built_in().keyboard(linux)
}

/// What each engine file of the runtime `linux` is (`voile`, `ours`, `ours-earlier`, `other`, `unreadable`),
/// by its path in the runtime.
pub fn components(linux: &Path) -> Vec<(String, &'static str)> {
    built_in().components(linux)
}

/// The pins of preparations, until their session's supervisor holds its own (by prefix).
fn preparation_pins() -> &'static std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Pin>> {
    static PINS: OnceLock<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Pin>>> = OnceLock::new();
    PINS.get_or_init(Default::default)
}

/// Puts our engine files in place in `linux` when they are for its release and no game runs on it, before a
/// game starts there, and pins the runtime as it then is for the launch of the installation whose prefix is
/// `holder` (released by [`release_preparation`] once its supervisor pinned it itself). A runtime it leaves
/// alone runs as it is (Voile's files, or a whole set of ours); `Err` when its files are not one complete set
/// a game may start on: the message names the paths and what to do.
pub(crate) fn apply(linux: &Path, holder: &Path) -> Result<Keyboard, Incoherent> {
    let (keyboard, pin) = built_in().prepare(linux)?;
    preparation_pins().lock().unwrap().insert(holder.to_owned(), pin);
    Ok(keyboard)
}

/// The preparation of `holder`'s launch no longer needs its pin (its supervisor holds one, or it ended).
pub fn release_preparation(holder: &Path) {
    preparation_pins().lock().unwrap().remove(holder);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    impl Patch {
        fn apply(&self, linux: &Path) -> Result<Keyboard, Incoherent> {
            self.prepare(linux).map(|(keyboard, _)| keyboard)
        }
        fn with(&self, linux: &Path, faults: Faults) -> Result<Keyboard, Incoherent> {
            match self.prepare_with(linux, faults) {
                Ok(done) => done.map(|(keyboard, _)| keyboard),
                Err(Crashed) => panic!("crashed"),
            }
        }
        fn crash(&self, linux: &Path, faults: Faults) {
            assert!(self.prepare_with(linux, faults).is_err(), "{faults:?} was to crash");
        }
    }

    /// Every file of the set is the same kind (never some of one set and some of another).
    fn one_set(linux: &Path) -> &'static str {
        let states: Vec<_> = patch().components(linux).into_iter().map(|(_, state)| state).collect();
        assert!(states.iter().all(|state| *state == states[0]), "a mixed set: {states:?}");
        states[0]
    }

    /// Nothing of a transaction is left beside the files or in the runtime.
    fn no_leftovers(linux: &Path) {
        assert!(!linux.join(JOURNAL).exists(), "the record is left");
        for folder in ["engine", "wine/lib/wine/x86_64-unix", "graphics/libexec"] {
            for entry in fs::read_dir(linux.join(folder)).unwrap().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                assert!(!name.starts_with(".obelisk-"), "{folder}/{name} is left");
            }
        }
    }

    fn journal(linux: &Path, phase: Phase, release: &str) -> Journal {
        let p = patch();
        Journal {
            id: "t1".to_owned(),
            phase,
            release: release.to_owned(),
            files: p.files.iter().map(|f| JournalFile { path: f.path.clone(), before: f.original.clone(), after: f.patched.clone() }).collect(),
        }
        .write_to(linux)
    }

    impl Journal {
        fn write_to(self, linux: &Path) -> Self {
            fs::write(linux.join(JOURNAL), serde_json::to_vec(&self).unwrap()).unwrap();
            self
        }
    }

    const RELEASE: &str = "v1.0.0-1";
    const WSD: &str = "engine/wsd";
    const DRIVER: &str = "wine/lib/wine/x86_64-unix/wineobelisk.so";
    const SERVER: &str = "graphics/libexec/virgl_render_server";
    const SERVER_X86: &str = "graphics/x86_64/libexec/virgl_render_server";
    const FEXDARWIN: &str = "engine/fexdarwin-run";

    /// A patch whose originals are `voile's wsd` and `voile's driver`.
    fn patch() -> Patch {
        let file = |path: &str, original: &[u8], bytes: &'static [u8]| File {
            path: path.to_owned(),
            original: sha(original),
            patched: sha(bytes),
            previous: vec![sha(b"our earlier server")],
            bytes,
        };
        Patch {
            release: RELEASE.to_owned(),
            files: vec![
                file(WSD, b"voile's wsd", b"our wsd"),
                file(DRIVER, b"voile's driver", b"our driver"),
                file(SERVER, b"voile's server", b"our server"),
            ],
        }
    }

    /// The Linux runtime of a release named `version`, with Voile's files.
    fn runtime(version: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let release = root.path().join(version);
        let linux = release.join("linux");
        fs::create_dir_all(linux.join("engine")).unwrap();
        fs::create_dir_all(linux.join("wine/lib/wine/x86_64-unix")).unwrap();
        fs::create_dir_all(linux.join("graphics/libexec")).unwrap();
        fs::write(
            release.join("runtime.json"),
            format!(r#"{{"version":"{version}","contract":1}}"#),
        )
        .unwrap();
        fs::write(linux.join(WSD), b"voile's wsd").unwrap();
        fs::set_permissions(linux.join(WSD), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(linux.join(DRIVER), b"voile's driver").unwrap();
        fs::write(linux.join(SERVER), b"voile's server").unwrap();
        fs::set_permissions(linux.join(SERVER), fs::Permissions::from_mode(0o755)).unwrap();
        (root, linux)
    }

    #[test]
    fn the_files_built_in_are_the_ones_the_manifest_names() {
        let patch = built_in();
        assert!(crate::voile::numbers(&patch.release).is_some());
        assert_eq!(patch.files.len(), FILES.len());
        assert_eq!(patch.files[0].path, WSD);
        assert_eq!(patch.files[1].path, DRIVER);
        assert_eq!(patch.files[2].path, SERVER);
        assert_eq!(patch.files[3].path, SERVER_X86);
        assert_eq!(patch.files[4].path, FEXDARWIN);
        for file in &patch.files {
            assert_eq!(sha(file.bytes), file.patched, "{}", file.path);
            assert_ne!(file.original, file.patched, "{}", file.path);
        }
    }

    #[test]
    fn our_files_replace_the_release_s_own_and_keep_their_mode() {
        let (_root, linux) = runtime(RELEASE);
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"our wsd");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"our server");
        for path in [WSD, SERVER] {
            let mode = fs::metadata(linux.join(path)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "{path}");
        }
        // Nothing is left beside them.
        let left: Vec<_> = fs::read_dir(linux.join("engine"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(left.len(), 1);
        assert!(!linux.join(".obelisk-original-wsd").exists());
        // And a second launch finds them in place.
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(patch().keyboard(&linux), Keyboard::Mac);
    }

    #[test]
    fn another_release_is_left_as_it_is() {
        let (_root, linux) = runtime("v1.0.0-2");
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
    }

    #[test]
    fn a_file_that_is_neither_voile_s_nor_ours_leaves_every_file_alone() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(DRIVER), b"someone else's driver").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        assert_eq!(
            fs::read(linux.join(DRIVER)).unwrap(),
            b"someone else's driver"
        );
    }

    #[test]
    fn a_missing_file_leaves_the_runtime_alone() {
        let (_root, linux) = runtime(RELEASE);
        fs::remove_file(linux.join(WSD)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert!(!linux.join(WSD).exists());
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
    }

    /// M12: a switch that fails part way puts every original back: never some of ours beside some of Voile's.
    #[test]
    fn a_switch_that_fails_part_way_leaves_voile_s_files_whole() {
        let (_root, linux) = runtime(RELEASE);
        assert_eq!(patch().with(&linux, Faults { switch: Some(1), ..Faults::default() }).unwrap(), Keyboard::Pending);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd", "the first one switched back");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"voile's server");
        assert!(patch().components(&linux).iter().all(|(_, state)| *state == "voile"));
        no_leftovers(&linux);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac, "the next launch puts all of them in");
        assert!(patch().components(&linux).iter().all(|(_, state)| *state == "ours"));
    }

    /// M12: a crash between the switches leaves the originals kept aside: the next launch puts them back first, then
    /// switches all at once.
    #[test]
    fn a_switch_a_crash_cut_short_is_undone_first() {
        let (_root, linux) = runtime(RELEASE);
        fs::hard_link(linux.join(WSD), aside_of(&linux.join(WSD))).unwrap();
        fs::write(linux.join(WSD).with_extension("tmp"), b"our wsd").unwrap();
        fs::rename(linux.join(WSD).with_extension("tmp"), linux.join(WSD)).unwrap();
        assert_eq!(patch().components(&linux)[0].1, "ours", "half switched");
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert!(!aside_of(&linux.join(WSD)).exists());
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
    }

    /// M12: a mixed installation says which file is which.
    #[test]
    fn a_mixed_installation_says_each_file_s_state() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        fs::write(linux.join(SERVER), b"someone else's server").unwrap();
        assert_eq!(patch().keyboard(&linux), Keyboard::Us);
        assert_eq!(patch().components(&linux), [(WSD.to_owned(), "ours"), (DRIVER.to_owned(), "voile"), (SERVER.to_owned(), "other")]);
    }

    #[test]
    fn a_patch_interrupted_between_its_files_is_completed() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
    }

    #[test]
    fn a_runtime_that_cannot_be_written_stays_pending() {
        let (_root, linux) = runtime(RELEASE);
        let engine = linux.join("engine");
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_runtime_without_a_manifest_is_left_as_it_is() {
        let (root, linux) = runtime(RELEASE);
        fs::remove_file(root.path().join(RELEASE).join("runtime.json")).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
    }

    #[test]
    fn a_render_server_that_is_neither_voile_s_nor_ours_leaves_every_file_alone() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(SERVER), b"someone else's server").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"someone else's server");
    }

    #[test]
    fn a_runtime_patched_for_the_keyboard_only_gets_the_render_server_too() {
        // What 0.1.2 and 0.1.3 left: our wsd and driver, Voile's render server.
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        fs::write(linux.join(DRIVER), b"our driver").unwrap();
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"our server");
    }

    /// The render servers built in are 64-bit Mach-O programs for arm64 and
    /// x86_64, each naming the render server's own path as Voile's does.
    #[test]
    fn the_render_servers_built_in_are_for_their_architectures() {
        let patch = built_in();
        let needle = b"/libexec/virgl_render_server";
        for (index, cpu) in [(2, 0x0100_000c_u32), (3, 0x0100_0007)] {
            let file = &patch.files[index];
            assert_eq!(file.bytes[..4], 0xfeed_facf_u32.to_le_bytes(), "{}", file.path);
            assert_eq!(file.bytes[4..8], cpu.to_le_bytes(), "{}", file.path);
            assert!(file.bytes.windows(needle.len()).any(|w| w == needle), "{}", file.path);
        }
    }

    /// With `OBELISK_ENGINE_PATCH_RUNTIME` naming the Linux runtime of a
    /// copy of the release the files are for, puts them in place there, for
    /// a test that then runs a Windows program through that copy.
    #[test]
    #[ignore = "writes to the runtime OBELISK_ENGINE_PATCH_RUNTIME names"]
    fn the_files_built_in_go_over_a_copy_of_their_release() {
        let linux = std::env::var_os("OBELISK_ENGINE_PATCH_RUNTIME").expect("a runtime to patch");
        let linux = Path::new(&linux);
        assert_eq!(keyboard(linux), Keyboard::Pending);
        assert_eq!(apply(linux, linux).unwrap(), Keyboard::Mac);
        release_preparation(linux);
        assert_eq!(keyboard(linux), Keyboard::Mac);
    }

    /// Finding 1 of the third verification: a file an earlier obelisk-cli put in place is ours, and is replaced
    /// by this one's like the release's own (0.1.5's render servers under 0.1.6 were "other" and blocked all).
    #[test]
    fn a_file_an_earlier_release_of_ours_put_there_is_replaced() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        fs::write(linux.join(DRIVER), b"our driver").unwrap();
        fs::write(linux.join(SERVER), b"our earlier server").unwrap();
        assert_eq!(patch().components(&linux)[2].1, "ours-earlier");
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"our server");
    }

    /// Finding 7: a switch that fails and whose undo fails too is reported, never taken for a coherent set.
    #[test]
    fn a_switch_whose_undo_fails_is_incoherent() {
        let (_root, linux) = runtime(RELEASE);
        let result = patch().with(&linux, Faults { switch: Some(1), undo: true, ..Faults::default() });
        let refused = result.unwrap_err();
        assert_eq!(refused.paths, [WSD, DRIVER, SERVER]);
        assert!(refused.reason.contains("reinstall Voile's release v1.0.0-1"), "{}", refused.reason);
        assert_eq!(patch().components(&linux)[0].1, "ours", "half switched, the record kept");
        assert!(linux.join(JOURNAL).exists());
        // The next launch puts the original back from where this transaction kept it, then patches all.
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(one_set(&linux), "ours");
        no_leftovers(&linux);
    }

    /// Finding 7: a file kept aside that is neither the release's nor an earlier one of ours is never installed;
    /// temporaries a crash left beside the files go.
    #[test]
    fn a_stale_or_foreign_file_kept_aside_is_not_installed() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(aside_of(&linux.join(WSD)), b"something else").unwrap();
        fs::write(linux.join("engine/.obelisk-AbC123"), b"half a file").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"our wsd");
        assert!(!aside_of(&linux.join(WSD)).exists());
        assert!(!linux.join("engine/.obelisk-AbC123").exists());
    }

    /// The files of every earlier obelisk-cli that patched this release, taken from its tag, over a copy of the
    /// release as Voile ships it (`OBELISK_PRISTINE_RELEASE`, the release folder): each set is replaced by ours.
    #[test]
    #[ignore = "needs OBELISK_PRISTINE_RELEASE and this repository's cli-v* tags"]
    fn every_earlier_release_s_files_are_replaced() {
        let pristine = std::env::var_os("OBELISK_PRISTINE_RELEASE").expect("a pristine release folder");
        let pristine = Path::new(&pristine);
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap();
        let tags = String::from_utf8(git(&["tag", "--list", "cli-v*"]).stdout).unwrap();
        let patch = built_in();
        let mut tried = 0;
        for tag in tags.lines() {
            let manifest = git(&["show", &format!("{tag}:crates/game-runtime/engine-patch/manifest.json")]);
            if !manifest.status.success() {
                continue; // before the engine patch
            }
            let old: Manifest = serde_json::from_slice(&manifest.stdout).unwrap();
            if old.voile != patch.release {
                continue;
            }
            let root = tempfile::tempdir().unwrap();
            let release = root.path().join(&patch.release);
            fs::create_dir_all(&release).unwrap();
            fs::copy(pristine.join("runtime.json"), release.join("runtime.json")).unwrap();
            let linux = release.join("linux");
            for file in &patch.files {
                let to = linux.join(&file.path);
                fs::create_dir_all(to.parent().unwrap()).unwrap();
                fs::copy(pristine.join("linux").join(&file.path), &to).unwrap();
            }
            for entry in &old.files {
                let bytes = git(&["show", &format!("{tag}:crates/game-runtime/engine-patch/{}", entry.source)]).stdout;
                assert_eq!(sha(&bytes), entry.patched, "{tag} {}", entry.source);
                fs::write(linux.join(&entry.path), bytes).unwrap();
            }
            assert_eq!(patch.keyboard(&linux), if old.files.len() == patch.files.len() && old.files.iter().all(|f| patch.files.iter().any(|p| p.patched == f.patched)) { Keyboard::Mac } else { Keyboard::Pending }, "{tag}");
            assert_eq!(patch.apply(&linux).unwrap(), Keyboard::Mac, "{tag}");
            for file in &patch.files {
                assert_eq!(crate::sha(&linux.join(&file.path)).unwrap(), file.patched, "{tag} {}", file.path);
            }
            tried += 1;
        }
        assert!(tried >= 4, "the cli-v0.1.2 to 0.1.6 tags");
    }
    /// Fourth verification, finding 2: a crash at any point of a transaction is ended at the next launch from
    /// its record alone, and that launch starts on one complete set.
    #[test]
    fn a_crash_in_any_phase_is_ended_from_the_record() {
        let crashes = [
            Faults { crash_after: Some(Phase::Staging), ..Faults::default() },
            Faults { crash_after: Some(Phase::Switching), ..Faults::default() },
            Faults { crash_after: Some(Phase::Switching), crash_switched: Some(1), ..Faults::default() },
            Faults { crash_after: Some(Phase::Switching), crash_switched: Some(2), ..Faults::default() },
            Faults { crash_after: Some(Phase::Committed), ..Faults::default() },
        ];
        for faults in crashes {
            let (_root, linux) = runtime(RELEASE);
            patch().crash(&linux, faults);
            assert!(linux.join(JOURNAL).exists(), "{faults:?}: the record stays after a crash");
            assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac, "{faults:?}");
            assert_eq!(one_set(&linux), "ours", "{faults:?}");
            no_leftovers(&linux);
        }
    }

    /// The same crashes followed by a failure of the next launch's own staging (a full disk): it ends the old
    /// transaction, then starts on the set it left (never ours / Voile's / ours).
    #[test]
    fn a_crash_then_a_full_disk_still_starts_on_one_set() {
        for faults in [
            Faults { crash_after: Some(Phase::Switching), crash_switched: Some(1), ..Faults::default() },
            Faults { crash_after: Some(Phase::Committed), ..Faults::default() },
        ] {
            let (_root, linux) = runtime(RELEASE);
            patch().crash(&linux, faults);
            let keyboard = patch().with(&linux, Faults { stage: Some(0), ..Faults::default() }).unwrap();
            let set = one_set(&linux);
            assert_eq!((keyboard, set), if faults.crash_after == Some(Phase::Committed) { (Keyboard::Mac, "ours") } else { (Keyboard::Pending, "voile") }, "{faults:?}");
            no_leftovers(&linux);
        }
    }

    /// The verifier's crash reproduction: an earlier obelisk-cli crashed after a successful switch, having
    /// removed some of its kept originals; one predecessor is still kept aside. Before, recovery installed
    /// it, then a staging failure returned Ok(Pending) with ours / Voile / ours. Now the kept copy is never
    /// installed over a file of the set, and the launch starts on all of ours.
    #[test]
    fn the_verifier_s_crash_reproduction_gives_one_set() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        fs::write(linux.join(DRIVER), b"our driver").unwrap();
        fs::write(linux.join(SERVER), b"our server").unwrap();
        fs::write(aside_of(&linux.join(DRIVER)), b"voile's driver").unwrap();
        assert_eq!(patch().with(&linux, Faults { stage: Some(0), ..Faults::default() }).unwrap(), Keyboard::Mac);
        assert_eq!(one_set(&linux), "ours");
        no_leftovers(&linux);
        // The same with an earlier set of ours in place and the staging failing: the earlier set, whole.
        let (_root, linux) = runtime(RELEASE);
        for path in [WSD, DRIVER, SERVER] {
            fs::write(linux.join(path), b"our earlier server").unwrap();
        }
        fs::write(aside_of(&linux.join(DRIVER)), b"voile's driver").unwrap();
        assert_eq!(patch().with(&linux, Faults { stage: Some(1), ..Faults::default() }).unwrap(), Keyboard::Pending);
        assert_eq!(one_set(&linux), "ours-earlier");
        no_leftovers(&linux);
    }

    /// A full disk at any of the record's writes: nothing changes, the launch runs on the set as it was.
    #[test]
    fn a_full_disk_at_any_record_write_changes_nothing() {
        for write in 0..3 {
            let (_root, linux) = runtime(RELEASE);
            let keyboard = patch().with(&linux, Faults { journal_write: Some(write), ..Faults::default() }).unwrap();
            assert_eq!(keyboard, Keyboard::Pending, "write {write}");
            assert_eq!(one_set(&linux), "voile", "write {write}");
            no_leftovers(&linux);
            assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        }
    }

    /// Finding 3: the release is checked before any recovery. A runtime that now declares another release
    /// gets nothing installed: not a kept original of an earlier cli, not a transaction's.
    #[test]
    fn the_release_is_checked_before_any_recovery() {
        let (_root, linux) = runtime("v1.0.0-2");
        fs::write(linux.join(WSD), b"v2's wsd").unwrap();
        fs::write(aside_of(&linux.join(WSD)), b"voile's wsd").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"v2's wsd");
        assert!(aside_of(&linux.join(WSD)).exists(), "another release's leftovers are not ours to touch");
        journal(&linux, Phase::Switching, RELEASE);
        fs::write(txn_name(&linux.join(WSD), "t1", "old"), b"voile's wsd").unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert!(refused.reason.contains("for Voile v1.0.0-1 was cut short in a runtime that is now v1.0.0-2"), "{}", refused.reason);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"v2's wsd");
    }

    /// Finding 3: a kept original that cannot be read is told apart from a foreign one, and neither is
    /// discarded or installed: the launch is refused, naming the path and what to do.
    #[test]
    fn an_unreadable_or_foreign_kept_original_is_refused_and_kept() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        journal(&linux, Phase::Switching, RELEASE);
        let old = txn_name(&linux.join(WSD), "t1", "old");
        fs::write(&old, b"voile's wsd").unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o000)).unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert_eq!(refused.paths, [WSD]);
        assert!(refused.reason.contains("cannot be read"), "{}", refused.reason);
        assert!(refused.reason.contains(&old.display().to_string()), "{}", refused.reason);
        fs::set_permissions(&old, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(old.exists());
        fs::write(&old, b"something else").unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert!(refused.reason.contains("is not the file this update replaced"), "{}", refused.reason);
        assert_eq!(fs::read(&old).unwrap(), b"something else");
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"our wsd");
        // Put right: the update is undone, then made.
        fs::write(&old, b"voile's wsd").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        no_leftovers(&linux);
    }

    /// An engine file that cannot be read is not taken for someone else's: the launch is refused.
    #[test]
    fn an_unreadable_engine_file_is_refused() {
        let (_root, linux) = runtime(RELEASE);
        fs::set_permissions(linux.join(DRIVER), fs::Permissions::from_mode(0o000)).unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert_eq!(refused.paths, [DRIVER]);
        assert_eq!(patch().components(&linux)[1].1, "unreadable");
        fs::set_permissions(linux.join(DRIVER), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
    }

    /// Finding 2/3: kept originals that cannot be removed after the commit cost nothing: the next launch only
    /// removes them, never undoes the update (no partial rollback, no WineIncomplete).
    #[test]
    fn kept_originals_left_after_the_commit_are_only_removed() {
        let (_root, linux) = runtime(RELEASE);
        assert_eq!(patch().with(&linux, Faults { remove_backups: true, ..Faults::default() }).unwrap(), Keyboard::Mac);
        assert!(linux.join(JOURNAL).exists());
        assert_eq!(patch().with(&linux, Faults { stage: Some(0), ..Faults::default() }).unwrap(), Keyboard::Mac);
        assert_eq!(one_set(&linux), "ours");
        no_leftovers(&linux);
    }

    /// Finding 4: while a session holds the runtime, no update changes its files; the next launch after it
    /// ended makes the update.
    #[test]
    fn a_running_session_pins_the_files() {
        let (_root, linux) = runtime(RELEASE);
        let session = Pin::hold(&linux).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending);
        assert_eq!(one_set(&linux), "voile");
        let (keyboard, second) = patch().prepare(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Pending);
        drop(session);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending, "the second launch's pin still holds");
        drop(second);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(one_set(&linux), "ours");
    }

    /// A preparation's pin lasts until its supervisor has its own (release_preparation).
    #[test]
    fn the_preparation_pin_is_held_until_released() {
        let (_root, linux) = runtime(RELEASE);
        let holder = linux.join("prefix-of-a-game");
        let (keyboard, pin) = patch().prepare(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Mac);
        preparation_pins().lock().unwrap().insert(holder.clone(), pin);
        let sessions = open_lock(&linux.join(SESSIONS_LOCK)).unwrap();
        assert!(matches!(sessions.try_lock(), Err(fs::TryLockError::WouldBlock)));
        release_preparation(&holder);
        assert!(sessions.try_lock().is_ok());
    }

    /// A runtime whose lock cannot be made (not writable by this user) starts on a complete set as it is, and
    /// refuses when an update was cut short there.
    #[test]
    fn without_the_lock_only_a_complete_set_starts() {
        let (_root, linux) = runtime(RELEASE);
        fs::set_permissions(&linux, fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending);
        assert_eq!(one_set(&linux), "voile");
        fs::set_permissions(&linux, fs::Permissions::from_mode(0o755)).unwrap();
        journal(&linux, Phase::Switching, RELEASE);
        fs::set_permissions(&linux, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(patch().apply(&linux).is_err());
        fs::set_permissions(&linux, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A record that cannot be read is refused, never guessed at.
    #[test]
    fn an_unreadable_record_is_refused() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(JOURNAL), b"{ half a rec").unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert_eq!(refused.paths, [JOURNAL]);
        assert_eq!(one_set(&linux), "voile");
    }

}
