//! The Orbiters fork's engine files. Voile's Linux engine takes every Mac
//! keyboard for a US one: its window server names a key by its place, and its
//! Wine driver has no layout. And Neptune's render server refused any one
//! resource update over 64 MiB, which VRChat sends seconds after an avatar
//! loads: the game's graphics went for good. Files of ours, built from
//! Voile's own sources plus our changes (`scripts/build-engine-patch.sh`,
//! `engine-patch/manifest.json`), replace the release's own before a game
//! starts on the engine: the window server then tells the layout selected on
//! the Mac, the driver gives the game its virtual keys and characters, and
//! the render servers (arm64 for DXMT, x86_64 for D3DMetal) take an update up
//! to 1 GiB.
//!
//! They are built for one release. In a runtime of that release (a *managed*
//! runtime) a game starts on one complete generation only: the exact digests
//! of every file, all as Voile ships them or all as one published obelisk-cli
//! left them (`Patch::complete`), proven under the runtime's lock on every
//! path that lets a launch go on, or the launch is refused ([`Incoherent`]).
//! Any other release, or a development runtime, is left as it is, and its
//! games run as Voile ships them.
//!
//! Files change as one recorded transaction and never under a running game.
//! One lock file for the whole library ([`Admitted`]) keeps every launch and
//! every change apart: a launch on either engine holds it shared from before
//! it picks a runtime to its game's exit ([`Pin`]), and whatever changes a
//! runtime holds it alone ([`Alone`]) from before it proves that nothing of
//! the library's engine runs, by a look at the Game sessions and at the
//! processes running its files (`live.rs`), to its last check. The locks live
//! beside the runtimes' folder, outside every folder an install replaces. The
//! two locks of one release ([`Locks`]) are kept for the builds that know only
//! them (0.1.9 and 0.1.10, and those before, inside the runtime).
use serde::{Deserialize, Serialize};
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Another set of engine files, which only a developer's build knows.
#[cfg(feature = "developer-timeline")]
pub mod developer;

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
    /// What each published obelisk-cli left in a runtime of that release.
    #[serde(default)]
    generations: Vec<Published>,
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

/// One published set of our files: the releases that carry it, and the SHA-256 of each path it changed. A
/// path it leaves out is as Voile ships it.
#[derive(Deserialize)]
struct Published {
    #[cfg_attr(not(test), allow(dead_code, reason = "for whoever reads the manifest"))]
    releases: Vec<String>,
    files: BTreeMap<String, String>,
}

/// Replacement files for the Linux engine of one Voile release.
pub(crate) struct Patch {
    release: String,
    files: Vec<File>,
    /// The sets an earlier published obelisk-cli left, each the SHA-256 of every file of `files`, in order.
    published: Vec<Vec<String>>,
    /// Proves that nothing of the engine of the library of these runtimes runs.
    idle: fn(&Path) -> Result<(), String>,
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
        of(manifest)
    })
}

#[cfg(feature = "developer-timeline")]
use developer::patch as of;

/// The files and sets of a manifest.
#[cfg(not(feature = "developer-timeline"))]
fn of(manifest: Manifest) -> Patch {
    let published = manifest
        .generations
        .iter()
        .map(|generation| {
            manifest
                .files
                .iter()
                .map(|entry| generation.files.get(&entry.path).unwrap_or(&entry.original).clone())
                .collect()
        })
        .collect();
    Patch {
        release: manifest.voile,
        published,
        idle: super::live::idle,
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
    /// A file neither the release's nor ours.
    Other,
    /// No file at that path.
    Missing,
    /// The file is there but cannot be read: never taken for "foreign", the launch is refused.
    Unreadable(String),
}

/// What ends a refusal, as the `error` event tells it (`docs/CLI.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Install the release again in its place (`obelisk-cli engine repair`).
    Repair,
    /// Quit the game that runs on this runtime, then start again.
    QuitGame,
    /// The lock of the runtime cannot be held: its folder's permissions or its disk.
    CheckStorage,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Repair => "repair",
            Self::QuitGame => "quit_game",
            Self::CheckStorage => "check_storage",
        }
    }
}

/// What ends a refusal that needs no repair.
fn advice(action: Action) -> &'static str {
    match action {
        Action::Repair => "reinstall Voile's release (obelisk-cli engine repair)",
        Action::QuitGame => "quit the game that runs on them, then start again",
        Action::CheckStorage => "the library's folder must be writable and its disk must take locks",
    }
}

/// The engine files are in a state no game may start on, and this program cannot repair it alone: the
/// paths, what is wrong and what the player (or the app) can do. Nothing was changed by the refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoherent {
    /// The paths, in the runtime, that are not what they must be.
    pub paths: Vec<String>,
    /// What is wrong, and what to do.
    pub reason: String,
    pub action: Action,
}

impl Incoherent {
    /// What a game cannot start without in `runtime` is gone from it, or not as its release ships it.
    pub(crate) fn missing(runtime: &Path, paths: Vec<String>) -> Self {
        let release = crate::voile::release_of(runtime);
        let release = release.as_deref().unwrap_or("in place");
        Self {
            paths,
            reason: format!(
                "an engine file is missing or damaged. No game starts on these engine files until this is resolved: reinstall Voile's release {release} (obelisk-cli engine repair)"
            ),
            action: Action::Repair,
        }
    }

    /// The localization key of a refusal, whose arguments are the reason, the action, then the paths.
    pub const KEY: &'static str = "backend.runtime.engineFiles";

    /// The refusal as the engine's failures cross to their caller: a localization key and its arguments.
    pub(crate) fn message(&self) -> String {
        let args: Vec<&str> = [self.reason.as_str(), self.action.name()]
            .into_iter()
            .chain(self.paths.iter().map(String::as_str))
            .collect();
        serde_json::json!({ "key": Self::KEY, "args": args }).to_string()
    }
}

impl std::fmt::Display for Incoherent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.reason, self.paths.join(", "))
    }
}

/// The locks of one runtime folder (`<voile>/<release>`, or the development runtime), by name: `<voile>.locks/`,
/// beside the runtimes' folder, so that nothing that replaces or removes a runtime takes a lock file with it.
struct Locks {
    folder: PathBuf,
    name: String,
    /// The runtime's Linux engine, where an obelisk-cli before 0.1.9 kept its locks: held too, while they are
    /// there to hold, so that neither ever changes the other's files.
    linux: PathBuf,
}

/// Makes the folder of the locks of `library`'s runtimes, as an install does: a library that can be written once
/// never needs to be again for its runtimes to be held.
pub fn provision(library: &Path) -> io::Result<()> {
    let root = library.join(super::ROOT);
    let folder = match root.canonicalize() {
        Ok(_) => locks_folder(&root)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => locks_folder_named(&library.canonicalize()?, super::ROOT.as_ref()),
        Err(error) => return Err(error),
    };
    fs::create_dir_all(folder)
}

fn locks_folder_named(library: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let mut locks = name.to_owned();
    locks.push(".locks");
    library.join(locks)
}

/// Where the locks of the runtimes in `root` are.
fn locks_folder(root: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    let name = root.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    Ok(locks_folder_named(root.parent().ok_or(io::ErrorKind::InvalidInput)?, name))
}

/// The same, made when it is not there.
fn open_locks_folder(root: &Path) -> io::Result<PathBuf> {
    let folder = locks_folder(root)?;
    fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// The one lock of a whole library, in its locks' folder: what is in there
/// nothing ever replaces.
const LIBRARY_LOCK: &str = "library.lock";

fn library_lock(root: &Path) -> io::Result<PathBuf> {
    Ok(open_locks_folder(root)?.join(LIBRARY_LOCK))
}

/// A launch admitted to the library whose runtimes are in `root`: it holds
/// the library's lock, alone when no other launch, game or change did, else
/// shared, which waits for a change under way.
#[derive(Debug)]
pub struct Admitted {
    file: fs::File,
    alone: bool,
}

impl Admitted {
    pub(crate) fn take(root: &Path) -> io::Result<Self> {
        let file = open_lock(&library_lock(root)?)?;
        let mut held_alone = alone(&file)?;
        if !held_alone {
            // Whoever changes the runtimes is waited for; then it may be alone.
            file.lock_shared()?;
            file.unlock()?;
            held_alone = alone(&file)?;
            if !held_alone {
                file.lock_shared()?;
            }
        }
        Ok(Self { file, alone: held_alone })
    }

    /// As `take`, or why no launch can be.
    pub(crate) fn launch(root: &Path) -> Result<Self, Incoherent> {
        Self::take(root).map_err(|error| Incoherent {
            paths: vec![root.display().to_string()],
            reason: format!(
                "the library's lock cannot be held ({error}). No game starts on these engine files until this is resolved: {}",
                advice(Action::CheckStorage)
            ),
            action: Action::CheckStorage,
        })
    }

    /// The lock shared with whoever else launches, as the launch's pin, with
    /// no runtime's own.
    pub(crate) fn pin(self) -> io::Result<Pin> {
        self.file.lock_shared()?;
        Ok(Pin { library: self.file, _runtime: None, _legacy: None })
    }
}

/// The library's lock, held alone: no launch is admitted and no game of one
/// admitted runs.
#[derive(Debug)]
pub(crate) struct Alone {
    _library: fs::File,
    root: PathBuf,
}

impl Alone {
    /// `None` while a launch, a game or another change holds it.
    pub(crate) fn hold(root: &Path) -> io::Result<Option<Self>> {
        let file = open_lock(&library_lock(root)?)?;
        Ok(alone(&file)?.then(|| Self { _library: file, root: root.to_owned() }))
    }

    /// Whether nothing of the library's engine is proven to run, told when so.
    pub(crate) fn idle(&self) -> bool {
        let running = super::live::idle(&self.root).err();
        if let Some(running) = &running {
            eprintln!("engine: the runtimes of {} are left as they are: {running}", self.root.display());
        }
        running.is_none()
    }

    /// Holds `runtime` too, as the builds before 0.1.11 do.
    pub(crate) fn release(&self, runtime: &Path) -> io::Result<Option<Release>> {
        let locks = Locks::of_runtime(runtime)?;
        let writer = locks.writer()?;
        let sessions = locks.sessions()?;
        if !alone(&sessions)? {
            return Ok(None);
        }
        let legacy = locks.legacy_sessions()?;
        if let Some(legacy) = &legacy
            && !alone(legacy)?
        {
            return Ok(None);
        }
        Ok(Some(Release { _writer: writer, _sessions: sessions, _legacy: legacy }))
    }
}

/// A runtime folder held against the builds before 0.1.11.
#[derive(Debug)]
pub(crate) struct Release {
    _writer: Writer,
    _sessions: fs::File,
    _legacy: Option<fs::File>,
}

impl Locks {
    /// Of the runtime folder `runtime`, which may not exist yet.
    fn of_runtime(runtime: &Path) -> io::Result<Self> {
        let root = runtime.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let name = runtime.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        Ok(Self {
            folder: open_locks_folder(root)?,
            name: name.to_string_lossy().into_owned(),
            linux: runtime.join(super::LINUX),
        })
    }

    /// Of the Linux engine `linux`, inside its runtime folder.
    fn of(linux: &Path) -> io::Result<Self> {
        Self::of_runtime(linux.parent().ok_or(io::ErrorKind::InvalidInput)?)
    }

    fn sessions_path(&self) -> PathBuf {
        self.folder.join(format!("{}.sessions.lock", self.name))
    }

    /// Held by whoever looks at or changes the files: waits its turn.
    fn writer(&self) -> io::Result<Writer> {
        let file = open_lock(&self.folder.join(format!("{}.writer.lock", self.name)))?;
        file.lock()?;
        // Only where one is: an older build made it, and only an older build waits on it.
        let legacy = open_legacy(&self.linux.join(LEGACY_WRITER))?;
        if let Some(legacy) = &legacy {
            legacy.lock()?;
        }
        Ok(Writer { _file: file, _legacy: legacy })
    }

    fn sessions(&self) -> io::Result<fs::File> {
        open_lock(&self.sessions_path())
    }

    /// An older build's session lock, where there is one.
    fn legacy_sessions(&self) -> io::Result<Option<fs::File>> {
        open_legacy(&self.linux.join(LEGACY_SESSIONS))
    }
}

/// An older build's lock file, to hold and never to write; `None` only where there is no such file. One that is
/// there and cannot be opened may be held by a game: that is an error.
fn open_legacy(path: &Path) -> io::Result<Option<fs::File>> {
    match fs::File::open(path) {
        Ok(file) => Ok(Some(file)),
        // (the engine's folder may not be there, or not be one)
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => Ok(None),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
struct Writer {
    _file: fs::File,
    _legacy: Option<fs::File>,
}

/// Whether `file`'s lock is ours alone now; `None` when there is nothing to hold.
fn alone(file: &fs::File) -> io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(fs::TryLockError::WouldBlock) => Ok(false),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

/// A library's runtimes held: while any process holds it no engine file of any of them changes (an update, a
/// repair or a removal needs every holder gone). One per launch: taken before its runtime is picked, carried by the
/// launch's own request, let go by whatever ends that launch (a failure, a cancellation), and inherited by the
/// session's supervisor as the same open lock, which it keeps until the game has exited: there is no moment without
/// it, whoever dies. On the Linux engine, whose files change, it holds the runtime's own lock too, for the builds
/// that know only that one.
#[derive(Debug)]
pub struct Pin {
    library: fs::File,
    _runtime: Option<fs::File>,
    _legacy: Option<fs::File>,
}

impl Pin {
    /// The held lock as a process's standard output: how a supervisor inherits it, and what drains its game's
    /// output once it is gone (`game_log`).
    pub(crate) fn stdio(&self) -> io::Result<std::process::Stdio> {
        Ok(self.library.try_clone()?.into())
    }

    /// The pin of a launch in `runtime` this process was started with, as its standard output, which is nowhere from
    /// then on.
    pub(crate) fn inherited(runtime: &Path, engine: crate::wine::Engine) -> io::Result<Self> {
        use std::os::fd::AsFd;
        let file = fs::File::from(io::stdout().as_fd().try_clone_to_owned()?);
        let pin = match engine {
            crate::wine::Engine::Linux => Self::adopt(runtime, file)?,
            crate::wine::Engine::Rosetta => Self::adopt_library(runtime.parent().ok_or(io::ErrorKind::InvalidInput)?, file)?,
        };
        let nowhere = fs::OpenOptions::new().write(true).open("/dev/null")?;
        rustix::stdio::dup2_stdout(&nowhere)?;
        Ok(pin)
    }

    /// `file` as the pin of the Linux engine `linux`.
    fn adopt(linux: &Path, file: fs::File) -> io::Result<Self> {
        let root = linux.parent().and_then(Path::parent).ok_or(io::ErrorKind::InvalidInput)?;
        let pin = Self::adopt_library(root, file)?;
        let locks = Locks::of(linux)?;
        let runtime = locks.sessions()?;
        held_shared(&runtime)?;
        let legacy = locks.legacy_sessions()?.filter(|file| file.try_lock_shared().is_ok());
        Ok(Self { _runtime: Some(runtime), _legacy: legacy, ..pin })
    }

    /// `file` as the pin of the library whose runtimes are in `root`: it is that library's own lock file, and its
    /// lock is held by it.
    fn adopt_library(root: &Path, file: fs::File) -> io::Result<Self> {
        let (given, expected) = (file.metadata()?, fs::symlink_metadata(library_lock(root)?)?);
        if !expected.is_file() || (given.dev(), given.ino()) != (expected.dev(), expected.ino()) {
            return Err(io::Error::other("not the library's lock"));
        }
        // Held already by what this process inherited: this takes nothing new, and fails when it was not.
        held_shared(&file)?;
        Ok(Self { library: file, _runtime: None, _legacy: None })
    }

    /// A pin of `linux`: waits for an update in progress (a few seconds at most).
    #[cfg(test)]
    pub(crate) fn hold(linux: &Path) -> io::Result<Self> {
        let root = linux.parent().and_then(Path::parent).ok_or(io::ErrorKind::InvalidInput)?;
        let pin = Admitted::take(root)?.pin()?;
        let runtime = Locks::of(linux)?.sessions()?;
        runtime.lock_shared()?;
        Ok(Self { _runtime: Some(runtime), ..pin })
    }
}

/// `file`'s lock, shared: at once, or not at all.
fn held_shared(file: &fs::File) -> io::Result<()> {
    file.try_lock_shared().map_err(|error| match error {
        fs::TryLockError::Error(error) => error,
        fs::TryLockError::WouldBlock => io::Error::other("the lock is not held"),
    })
}

/// A runtime folder nobody else starts in or changes, held while one is put there, replaced or removed.
#[derive(Debug)]
pub(crate) struct Replacing {
    _library: Alone,
    _release: Release,
}

impl Replacing {
    /// Holds `runtime` (`<voile>/<release>`) to replace or remove what is there; `None` while a game's session
    /// holds the library, anything of its engine runs, or that cannot be told.
    pub(crate) fn hold(runtime: &Path) -> io::Result<Option<Self>> {
        Ok(Self::free(runtime)?.filter(Self::idle))
    }

    /// Whether nothing of the library's engine is proven to run, told when so.
    pub(crate) fn idle(&self) -> bool {
        self._library.idle()
    }

    /// Holds `runtime`, where nothing is yet; `None` while a launch or a game holds the library.
    pub(crate) fn free(runtime: &Path) -> io::Result<Option<Self>> {
        let Some(library) = Alone::hold(runtime.parent().ok_or(io::ErrorKind::InvalidInput)?)? else {
            return Ok(None);
        };
        let release = library.release(runtime)?;
        Ok(release.map(|release| Self { _library: library, _release: release }))
    }
}

/// Held by the one process that installs into `root`: two never remove each other's unfinished work.
pub(crate) fn installing(root: &Path) -> io::Result<Option<fs::File>> {
    let file = open_lock(&open_locks_folder(root)?.join("install.lock"))?;
    Ok(alone(&file)?.then_some(file))
}

/// Where an obelisk-cli before 0.1.9 kept its locks, in the Linux engine's folder.
const LEGACY_WRITER: &str = ".obelisk-engine-patch.lock";
const LEGACY_SESSIONS: &str = ".obelisk-engine-sessions.lock";
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

    /// Whether `sha` is a file this path may have had: the release's, ours, or an earlier one of ours.
    fn known(&self, sha: &str) -> bool {
        self.found_sha(sha) != Found::Other
    }

    /// Whether `sha` is ours, this build's or an earlier one's.
    fn ours(&self, sha: &str) -> bool {
        matches!(self.found_sha(sha), Found::Patched | Found::Previous)
    }
}

/// The SHA-256 of the file at `path`; `None` when there is nothing there. Anything else there but a file of
/// its own (a link leads to what may change under a game) cannot be read as one.
fn read_sha(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if !metadata.is_file() => return Err("it is not a regular file".to_owned()),
        Ok(_) => {}
    }
    match crate::sha(path) {
        Ok(sha) => Ok(Some(sha)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// The lock file at `path`, made only where there is none: a lock is held on what can only be read too, and one
/// that is there may be.
fn open_lock(path: &Path) -> io::Result<fs::File> {
    match fs::metadata(path) {
        Ok(found) if found.is_file() => fs::File::open(path),
        Ok(_) => Err(io::Error::other("the lock is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::OpenOptions::new().read(true).write(true).create_new(true).open(path) {
                // (made meanwhile)
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => fs::File::open(path),
                made => made,
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
thread_local! {
    /// How many directory syncs succeed before every next one of this thread fails.
    static SYNCS_LEFT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Makes the names in the folder of `path` durable.
fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(test)]
    if let Some(left) = SYNCS_LEFT.get() {
        if left == 0 {
            return Err(io::Error::other("a test's failed sync"));
        }
        SYNCS_LEFT.set(Some(left - 1));
    }
    fs::File::open(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?.sync_all()
}

/// The names a transaction uses beside a destination: tied to the transaction and to that destination.
fn txn_name(path: &Path, id: &str, what: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".obelisk-txn-{id}-{what}-{name}"))
}

/// Where an obelisk-cli before 0.1.8 kept the original of `path` during a switch (no transaction record).
fn aside_of(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".obelisk-original-{name}"))
}

impl Patch {
    fn found(&self, linux: &Path) -> Vec<Found> {
        self.files
            .iter()
            .map(|file| match read_sha(&linux.join(&file.path)) {
                Ok(Some(sha)) => file.found_sha(&sha),
                Ok(None) => Found::Missing,
                Err(error) => Found::Unreadable(error),
            })
            .collect()
    }

    fn keyboard_of(&self, linux: &Path, found: &[Found]) -> Keyboard {
        if crate::voile::release_of(linux).as_deref() != Some(self.release.as_str()) {
            return Keyboard::Us;
        }
        if found.iter().any(|found| matches!(found, Found::Other | Found::Missing | Found::Unreadable(_))) {
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
                    Found::Other | Found::Missing => "other",
                    Found::Unreadable(_) => "unreadable",
                };
                (file.path.clone(), state)
            })
            .collect()
    }

    /// The sets a game may start on: Voile's own files, ours, and what each published build left.
    fn generations(&self) -> Vec<Vec<&str>> {
        let voile = self.files.iter().map(|file| file.original.as_str()).collect();
        let ours = self.files.iter().map(|file| file.patched.as_str()).collect();
        let published = self.published.iter().map(|set| set.iter().map(String::as_str).collect());
        [ours, voile].into_iter().chain(published).collect()
    }

    /// Proves that the files of `linux` are one complete released generation, digest by digest: what every
    /// path that lets a game start on a managed runtime ends with. `Err` names the files that are not of the
    /// set they come closest to.
    fn complete(&self, linux: &Path) -> Result<Keyboard, Incoherent> {
        let mut shas = Vec::new();
        for file in &self.files {
            let sha = read_sha(&linux.join(&file.path)).map_err(|error| {
                self.refuse(vec![file.path.clone()], &format!("an engine file cannot be read ({error})"), Action::Repair)
            })?;
            shas.push(sha.unwrap_or_default());
        }
        let generations = self.generations();
        let differing = |set: &Vec<&str>| -> Vec<String> {
            let pairs = self.files.iter().zip(shas.iter().zip(set));
            pairs.filter(|(_, (sha, of))| sha != *of).map(|(file, _)| file.path.clone()).collect()
        };
        // (the first is ours, the second Voile's: the closest of equals is the one expected)
        let closest = generations.iter().map(differing).enumerate().min_by_key(|(_, files)| files.len());
        let (index, closest) = closest.unwrap_or_default();
        if closest.is_empty() {
            return Ok(if index == 0 { Keyboard::Mac } else { Keyboard::Pending });
        }
        let missing = shas.iter().any(String::is_empty);
        let problem = match missing {
            true => "an engine file is missing",
            false => "the engine files are not all of one release",
        };
        Err(self.refuse(closest, problem, Action::Repair))
    }

    fn refuse(&self, paths: Vec<String>, problem: &str, action: Action) -> Incoherent {
        let advice = match action {
            Action::Repair => format!("reinstall Voile's release {} (obelisk-cli engine repair)", self.release),
            action => advice(action).to_owned(),
        };
        Incoherent {
            paths,
            reason: format!("{problem}. No game starts on these engine files until this is resolved: {advice}"),
            action,
        }
    }

    /// Our files over the release's own (or an earlier obelisk-cli's) in `linux`, all of them or none, when
    /// every one is the release's or ours and no session is running in the library; returns the keyboard
    /// the launch gets and a pin that keeps the runtimes' files as they are until it is dropped. `Err` when
    /// the files are not one complete set this program can start on or repair (nothing is changed then), or
    /// when they cannot be pinned.
    fn prepare(&self, linux: &Path, admitted: Admitted) -> Result<(Keyboard, Pin), Incoherent> {
        match self.prepare_with(linux, admitted, Faults::default()) {
            Ok(done) => done,
            Err(Crashed) => unreachable!("no fault was injected"),
        }
    }

    fn prepare_with(
        &self,
        linux: &Path,
        admitted: Admitted,
        faults: Faults,
    ) -> Result<Result<(Keyboard, Pin), Incoherent>, Crashed> {
        let unlocked = |error: io::Error| {
            self.refuse(
                vec![linux.display().to_string()],
                &format!("the runtime's lock cannot be held ({error})"),
                Action::CheckStorage,
            )
        };
        let locks = match Locks::of(linux) {
            Ok(locks) => locks,
            Err(error) => return Ok(Err(unlocked(error))),
        };
        // Writers (and the installer) are serialised by the writer lock; sessions hold the sessions lock shared.
        let held = locks.writer().and_then(|writer| Ok((writer, locks.sessions()?)));
        let (_writer, sessions) = match held {
            Ok(held) => held,
            Err(error) => return Ok(Err(unlocked(error))),
        };
        let legacy = match locks.legacy_sessions() {
            Ok(legacy) => legacy,
            Err(error) => return Ok(Err(unlocked(error))),
        };
        let exclusive = match alone(&sessions) {
            Ok(alone_here) => admitted.alone && alone_here && legacy.as_ref().is_none_or(|legacy| alone(legacy).unwrap_or(false)),
            Err(error) => return Ok(Err(unlocked(error))),
        };
        let done = self.under_locks(linux, exclusive, faults)?;
        // Hand over from the writer's locks to the session's: no update can come between (we still hold the
        // release's writer lock, which every writer and the installer need).
        Ok(done.and_then(|keyboard| {
            admitted.file.lock_shared().map_err(unlocked)?;
            let _ = sessions.unlock();
            sessions.lock_shared().map_err(unlocked)?;
            // An older build changes only files it knows, and waits on its own lock alone: where it would find some
            // to change (ours are all in place for it too), its lock is held with this one, made if need be.
            let legacy = match legacy {
                None if keyboard == Keyboard::Pending => open_lock(&linux.join(LEGACY_SESSIONS)).ok(),
                legacy => legacy,
            };
            if let Some(legacy) = &legacy {
                let _ = legacy.unlock();
                legacy.lock_shared().map_err(unlocked)?;
                if keyboard != Keyboard::Us {
                    self.complete(linux)?;
                }
            }
            Ok((keyboard, Pin { library: admitted.file, _runtime: Some(sessions), _legacy: legacy }))
        }))
    }

    /// The record of a transaction in `linux`, one this program can have written for the release there.
    fn journal(&self, linux: &Path, release: Option<&str>) -> Result<Option<Journal>, Incoherent> {
        let unreadable = |error: String| {
            let what = format!("the record of an engine update cannot be read ({error})");
            self.refuse(vec![JOURNAL.to_owned()], &what, Action::Repair)
        };
        let path = linux.join(JOURNAL);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(unreadable(error.to_string())),
            Ok(_) => {}
        }
        let bytes = fs::read(&path).map_err(|error| unreadable(error.to_string()))?;
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|error| unreadable(error.to_string()))?;
        if release != Some(journal.release.as_str()) {
            return Err(self.refuse(
                vec![JOURNAL.to_owned()],
                &format!(
                    "an engine update for Voile {} was cut short in a runtime that is now {}",
                    journal.release,
                    release.unwrap_or("of no known release")
                ),
                Action::Repair,
            ));
        }
        self.valid(&journal)?;
        Ok(Some(journal))
    }

    /// `alone`: the library's lock is ours alone, and no session of an earlier build holds the runtime's.
    fn under_locks(&self, linux: &Path, alone: bool, faults: Faults) -> Result<Result<Keyboard, Incoherent>, Crashed> {
        // The files change only once nothing of the library's engine is proven to run, looked at once and only when
        // something is to change: a launch on files that need nothing waits for nothing.
        let proven = OnceCell::new();
        let exclusive = || {
            *proven.get_or_init(|| {
                let root = linux.parent().and_then(Path::parent);
                let running = match root {
                    Some(root) if alone => (self.idle)(root).err(),
                    _ => Some("another launch holds it".to_owned()),
                };
                if let Some(running) = &running {
                    eprintln!("engine patch: the files stay as they are: {running}");
                }
                running.is_none()
            })
        };
        // 1. The release first: nothing of another release is recovered or installed.
        let release = crate::voile::release_of(linux);
        let journal = match self.journal(linux, release.as_deref()) {
            Ok(journal) => journal,
            Err(refused) => return Ok(Err(refused)),
        };
        let managed = release.as_deref() == Some(self.release.as_str());
        if let Some(journal) = &journal {
            if !exclusive() {
                // Only its cleanup may be pending: a complete set starts, and that waits.
                if managed && let Ok(keyboard) = self.complete(linux) {
                    return Ok(Ok(keyboard));
                }
                return Ok(Err(self.refuse(
                    vec![JOURNAL.to_owned()],
                    "an engine update was cut short while another game runs on this runtime",
                    Action::QuitGame,
                )));
            }
            if let Err(incoherent) = self.recover(linux, journal) {
                return Ok(Err(incoherent));
            }
        }
        if !managed {
            // Another release, or a development runtime: as it is, unless files of ours are in it.
            let ours: Vec<String> = self
                .files
                .iter()
                .filter(|file| matches!(read_sha(&linux.join(&file.path)), Ok(Some(sha)) if file.ours(&sha) && sha != file.original))
                .map(|file| file.path.clone())
                .collect();
            if release.is_some() && !ours.is_empty() {
                return Ok(Err(self.refuse(ours, "engine files for another Voile release are in this one", Action::Repair)));
            }
            return Ok(Ok(Keyboard::Us));
        }
        let mut found = self.found(linux);
        if found.contains(&Found::Missing) && exclusive() {
            self.restore_sole_copies(linux);
            found = self.found(linux);
        }
        if self.keyboard_of(linux, &found) == Keyboard::Pending
            && exclusive()
            && let Err(incoherent) = self.transaction(linux, &found, faults)?
        {
            return Ok(Err(incoherent));
        }
        // Whatever happened above (nothing, a switch, a full disk, another session's lock): one complete
        // generation, or no launch.
        let complete = self.complete(linux);
        if complete.is_ok() && !linux.join(JOURNAL).exists() {
            let left = self.leftovers(linux);
            if !left.is_empty() && exclusive() {
                for file in left {
                    if fs::remove_file(&file).is_err() {
                        eprintln!("engine patch: {} cannot be removed; it is not used", file.display());
                    }
                }
            }
        }
        Ok(complete)
    }

    /// A file an obelisk-cli before 0.1.8 kept aside (no record) goes back where it is the sole copy of a file
    /// that is gone, and never over a file that is there.
    fn restore_sole_copies(&self, linux: &Path) {
        for file in &self.files {
            let path = linux.join(&file.path);
            let aside = aside_of(&path);
            if read_sha(&path) == Ok(None)
                && matches!(read_sha(&aside), Ok(Some(sha)) if file.known(&sha))
                && fs::hard_link(&aside, &path).is_ok()
            {
                let _ = fs::remove_file(&aside);
            }
        }
    }

    /// What a complete set no longer needs: files an obelisk-cli before 0.1.8 kept aside (never installed while the
    /// file at the destination is one of the set), transaction files of no pending transaction, and old
    /// temporaries. A set that is not complete keeps all of them: they may hold its only good copy.
    fn leftovers(&self, linux: &Path) -> Vec<PathBuf> {
        let mut left = Vec::new();
        let mut folders = BTreeSet::new();
        for file in &self.files {
            let path = linux.join(&file.path);
            let aside = aside_of(&path);
            if aside.is_file() {
                left.push(aside);
            }
            if let Some(folder) = path.parent() {
                folders.insert(folder.to_owned());
            }
        }
        let temp = linux.join(format!("{JOURNAL}.new"));
        if temp.exists() {
            left.push(temp);
        }
        for folder in folders {
            for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let leftover = name.starts_with(".obelisk-")
                    && !name.starts_with(".obelisk-original-")
                    && !name.starts_with(".obelisk-engine-");
                if leftover {
                    left.push(entry.path());
                }
            }
        }
        left
    }

    fn write_journal(&self, linux: &Path, journal: &Journal, writes: &mut usize, faults: Faults) -> io::Result<()> {
        let index = *writes;
        *writes += 1;
        if faults.journal_write == Some(index) {
            return Err(io::Error::other("a test's full disk"));
        }
        let path = linux.join(JOURNAL);
        let temp = linux.join(format!("{JOURNAL}.new"));
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(journal).map_err(io::Error::other)?)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        sync_dir(&path)
    }

    /// Removes the record of transaction `id`, and no other's.
    fn abandon_journal(linux: &Path, id: &str) {
        let written = fs::read(linux.join(JOURNAL)).ok().and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok());
        if written.is_some_and(|journal| journal.id == id) {
            let _ = Self::remove_journal(linux);
        }
    }

    fn remove_journal(linux: &Path) -> io::Result<()> {
        let path = linux.join(JOURNAL);
        match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            // A removal that is not durable yet brings back, after a crash, a record whose files are all in
            // place: it is then only removed again.
            _ => {
                let _ = sync_dir(&path);
                Ok(())
            }
        }
    }

    /// Puts ours in place over `found`, all of it known. `Ok` leaves the files as they were or all ours
    /// (the caller proves which); `Err` when a switch could not be undone.
    fn transaction(&self, linux: &Path, found: &[Found], faults: Faults) -> Result<Result<(), Incoherent>, Crashed> {
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
            match read_sha(&linux.join(&file.path)) {
                Ok(Some(before)) if file.known(&before) => {
                    journal.files.push(JournalFile { path: file.path.clone(), before, after: file.patched.clone() });
                }
                // changed since it was looked at: nothing is done here, the proof that follows tells
                _ => return Ok(Ok(())),
            }
        }
        let mut writes = 0;
        // A journal that cannot be written (a full disk): nothing changes, the launch runs on the set as it is.
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            Self::abandon_journal(linux, &id);
            return Ok(Ok(()));
        }
        if faults.crash_after == Some(Phase::Staging) {
            return Err(Crashed);
        }
        let switch: Vec<&File> = self.files.iter().zip(found).filter(|(_, f)| **f != Found::Patched).map(|(f, _)| f).collect();
        let abandon = |linux: &Path| {
            for file in &self.files {
                let path = linux.join(&file.path);
                let _ = fs::remove_file(txn_name(&path, &id, "new"));
                let _ = fs::remove_file(txn_name(&path, &id, "old"));
            }
            Self::abandon_journal(linux, &id);
        };
        for (index, file) in switch.iter().enumerate() {
            let path = linux.join(&file.path);
            let staged = if faults.stage == Some(index) {
                Err(io::Error::other("a test's full disk"))
            } else {
                stage(&path, &txn_name(&path, &id, "new"), file.bytes, &file.patched)
            };
            // Both names are durable before the record says that files are switched from them.
            let kept = staged.and_then(|()| fs::hard_link(&path, txn_name(&path, &id, "old"))).and_then(|()| sync_dir(&path));
            if kept.is_err() {
                abandon(linux);
                return Ok(Ok(()));
            }
        }
        journal.phase = Phase::Switching;
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            abandon(linux);
            return Ok(Ok(()));
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
                Err(io::Error::other("a test's failure"))
            } else {
                fs::rename(txn_name(&path, &id, "new"), &path).and_then(|()| sync_dir(&path))
            };
            if done.is_err() {
                return Ok(self.roll_back_now(linux, &journal, faults.undo));
            }
        }
        // Every file checked against the record before the commit is.
        if self.files.iter().any(|file| read_sha(&linux.join(&file.path)).ok().flatten().as_deref() != Some(file.patched.as_str())) {
            return Ok(self.roll_back_now(linux, &journal, faults.undo));
        }
        journal.phase = Phase::Committed;
        if self.write_journal(linux, &journal, &mut writes, faults).is_err() {
            // The record in place, when it is the one written (only its name may not be durable): the files are all
            // ours and durable, and either record ends in one set from the originals kept. Else it still says
            // Switching: undo as from it.
            let written = fs::read(linux.join(JOURNAL)).ok().and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok());
            if written.as_ref() == Some(&journal) {
                return Ok(Ok(()));
            }
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
        Ok(Ok(()))
    }

    /// A switch failed: every file back to its `before`, checked; the record goes when that worked, and stays
    /// (for the next launch, which retries) when it did not.
    fn roll_back_now(&self, linux: &Path, journal: &Journal, fail: bool) -> Result<(), Incoherent> {
        if fail {
            return Err(self.refuse(
                journal.files.iter().map(|f| f.path.clone()).collect(),
                "an engine update failed part way and could not be undone (a test's failure)",
                Action::Repair,
            ));
        }
        self.recover(linux, journal)
    }

    /// The record is one this program can have written: its id names files and nothing else, every path of the
    /// set is there exactly once and no other, each `before` is a file that path may have had, and the files
    /// `after` are one published generation: an update puts nothing else in place. Nothing is read or changed
    /// from a record that is not.
    fn valid(&self, journal: &Journal) -> Result<(), Incoherent> {
        let refuse = |what: String| {
            Err(self.refuse(vec![JOURNAL.to_owned()], &format!("the record of an engine update is not one: {what}"), Action::Repair))
        };
        let id_names = !journal.id.is_empty()
            && journal.id.len() <= 64
            && journal.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !id_names {
            return refuse("its id".to_owned());
        }
        for file in &self.files {
            let mut records = journal.files.iter().filter(|record| record.path == file.path);
            let (Some(record), None) = (records.next(), records.next()) else {
                return refuse(format!("{} is not in it once", file.path));
            };
            if !file.known(&record.before) {
                return refuse(format!("the file before, at {}", file.path));
            }
            // (a developer's build also puts back files that only the timeline generation changes: Voile's own)
            let put_back = cfg!(feature = "developer-timeline") && record.after == file.original;
            if !file.ours(&record.after) && !put_back {
                return refuse(format!("the file after, at {}", file.path));
            }
        }
        if journal.files.iter().any(|record| !self.files.iter().any(|file| file.path == record.path)) {
            return refuse("a path that is not of the set".to_owned());
        }
        match self.generation(journal, |record| &record.after) {
            true => Ok(()),
            false => refuse("the files after it are no published set".to_owned()),
        }
    }

    /// Whether the files a (valid) record names by `which` are one published generation.
    fn generation(&self, journal: &Journal, which: impl Fn(&JournalFile) -> &String) -> bool {
        let of = |file: &File| journal.files.iter().find(|record| record.path == file.path).map(&which);
        let files: Option<Vec<&str>> = self.files.iter().map(|file| of(file).map(String::as_str)).collect();
        files.is_some_and(|files| self.generations().contains(&files))
    }

    /// Ends a transaction a crash (or a failure) left: `Committed` only removes its kept originals; any other
    /// phase puts every file back to its `before` from the original kept for this transaction and this
    /// destination, checked by digest. Everything it needs is read and checked before the first file changes:
    /// a file or a kept original that is unreadable, missing, or not the digest the record names, is never
    /// guessed at, and nothing else is touched for it: refused, naming it, the record and the kept originals
    /// left where they are. The kept originals and the record go only once the files are a published generation,
    /// verified; any other set keeps them all (a later update that completes it, or a repair, ends them).
    fn recover(&self, linux: &Path, journal: &Journal) -> Result<(), Incoherent> {
        self.valid(journal)?;
        let mut problems = Vec::new();
        let mut why = String::new();
        let mut back = Vec::new();
        for record in &journal.files {
            let path = linux.join(&record.path);
            let old = txn_name(&path, &journal.id, "old");
            let mut problem = |what: String| {
                problems.push(record.path.clone());
                why = what;
            };
            let current = match read_sha(&path) {
                Ok(sha) => sha,
                Err(error) => {
                    problem(format!("{} cannot be read ({error})", path.display()));
                    continue;
                }
            };
            if journal.phase == Phase::Committed {
                if current.as_deref() != Some(record.after.as_str()) {
                    problem(format!("{} changed after an engine update was committed", path.display()));
                }
                continue;
            }
            if current.as_deref() == Some(record.before.as_str()) {
                continue; // not switched
            }
            if current.as_deref() != Some(record.after.as_str()) {
                problem(format!("{} is neither the file before the update nor ours", path.display()));
                continue;
            }
            match read_sha(&old) {
                Ok(Some(sha)) if sha == record.before => back.push((old, path, record)),
                Ok(Some(_)) => problem(format!("{} is not the file this update replaced", old.display())),
                Ok(None) => problem(format!("the original of {} kept for this update ({}) is missing", path.display(), old.display())),
                Err(error) => problem(format!(
                    "the original of {} kept for this update ({}) cannot be read ({error})",
                    path.display(),
                    old.display()
                )),
            }
        }
        let refuse = |paths: Vec<String>, why: &str| {
            Err(self.refuse(paths, &format!("an engine update cut short cannot be ended: {why}"), Action::Repair))
        };
        if !problems.is_empty() {
            return refuse(problems, &why);
        }
        for (old, path, record) in back {
            // A copy goes back, the kept original stays until every file is back and checked.
            let restored = txn_name(&path, &journal.id, "back");
            let _ = fs::remove_file(&restored);
            let done = fs::hard_link(&old, &restored).and_then(|()| fs::rename(&restored, &path)).and_then(|()| sync_dir(&path));
            if let Err(error) = done {
                let why = format!("{} cannot be put back from {} ({error})", path.display(), old.display());
                return refuse(vec![record.path.clone()], &why);
            }
        }
        for record in &journal.files {
            let path = linux.join(&record.path);
            let expected = if journal.phase == Phase::Committed { &record.after } else { &record.before };
            if read_sha(&path).ok().flatten().as_ref() != Some(expected) {
                return refuse(vec![record.path.clone()], &format!("{} is not what the record says", path.display()));
            }
            // Durable before the kept originals go, whichever run put it there.
            if let Err(error) = sync_dir(&path) {
                return refuse(vec![record.path.clone()], &format!("{} is not durable ({error})", path.display()));
            }
        }
        if !self.generation(journal, |record| if journal.phase == Phase::Committed { &record.after } else { &record.before }) {
            return Ok(());
        }
        // Every file is its `before` (or, committed, its `after`): the transaction's own files go, then the record.
        for record in &journal.files {
            let path = linux.join(&record.path);
            for what in ["old", "new", "back"] {
                let file = txn_name(&path, &journal.id, what);
                if file.exists() && fs::remove_file(&file).is_err() && journal.phase != Phase::Committed {
                    eprintln!("engine patch: {} cannot be removed", file.display());
                }
            }
        }
        if Self::remove_journal(linux).is_err() && journal.phase != Phase::Committed {
            return Err(self.refuse(vec![JOURNAL.to_owned()], "the record of an ended engine update cannot be removed", Action::CheckStorage));
        }
        Ok(())
    }

    /// Whether the Linux engine `linux` an install laid, of `release`, holds Voile's own files, every one: checked
    /// before it replaces anything, and again in its place. A release the files are not for has nothing to check.
    fn pristine(&self, linux: &Path, release: &str) -> bool {
        release != self.release
            || self.files.iter().all(|file| matches!(read_sha(&linux.join(&file.path)), Ok(Some(sha)) if sha == file.original))
    }
}

/// Our file written at `to` (beside `path`) with `path`'s mode, synced, and checked against `sha`.
fn stage(path: &Path, to: &Path, bytes: &[u8], sha: &str) -> io::Result<()> {
    let mode = fs::metadata(path)?.permissions().mode();
    let _ = fs::remove_file(to);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(to)?;
    file.write_all(bytes)?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    if crate::sha(to).ok().as_deref() != Some(sha) {
        return Err(io::Error::other("the staged file is not ours"));
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

/// Whether the Linux engine `linux` an install laid, of `release`, is whole, as its release ships it.
pub(crate) fn pristine(linux: &Path, release: &str) -> bool {
    built_in().pristine(linux, release)
}

/// The one Voile release the files are for: it carries a Linux engine, whatever is left of it.
pub(crate) fn release() -> &'static str {
    &built_in().release
}

/// Puts our engine files in place in `linux` when they are for its release and no game runs on it, before a
/// game starts there, and pins the runtime as it then is for that launch: the pin goes where the launch goes,
/// and its drop lets go of the runtime. A runtime of another release runs as it is (Voile's files); `Err` when
/// the files are not one complete set a game may start on, or cannot be pinned: the refusal names the paths
/// and what to do.
pub fn apply(linux: &Path) -> Result<(Keyboard, Pin), Incoherent> {
    apply_admitted(linux, Admitted::launch(linux.parent().and_then(Path::parent).unwrap_or(linux))?)
}

/// The same for a launch that was admitted before it picked `linux`.
pub(crate) fn apply_admitted(linux: &Path, admitted: Admitted) -> Result<(Keyboard, Pin), Incoherent> {
    built_in().prepare(linux, admitted).and_then(|held| asked_for(linux).map(|()| held))
}

/// Whether the files in `linux`, proven to be a set, are the one this process is to start games on.
#[cfg(feature = "developer-timeline")]
fn asked_for(linux: &Path) -> Result<(), Incoherent> {
    developer::proven(built_in(), linux)
}

#[cfg(not(feature = "developer-timeline"))]
fn asked_for(_: &Path) -> Result<(), Incoherent> {
    Ok(())
}

/// Whether the file at `path` of the Linux engine `linux` is one of ours to patch: its digest is what proves it.
pub(crate) fn proven(path: &str) -> bool {
    built_in().files.iter().any(|file| file.path == path)
}

/// Puts this build's own engine files in `linux`, each with the mode of the one it replaces.
#[cfg(test)]
pub(crate) fn lay_ours(linux: &Path) {
    for file in &built_in().files {
        fs::write(linux.join(&file.path), file.bytes).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// For developer.rs's tests.
    #[cfg(feature = "developer-timeline")]
    pub(super) fn prepared_for_tests(patch: &Patch, linux: &Path) -> Result<(Keyboard, Pin), Incoherent> {
        patch.prepared(linux)
    }
    #[cfg(feature = "developer-timeline")]
    pub(super) fn prepared_with_for_tests(patch: &Patch, linux: &Path, faults: Faults) -> Result<Result<(Keyboard, Pin), Incoherent>, Crashed> {
        patch.prepare_with(linux, Patch::admitted(linux).unwrap(), faults)
    }

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    impl Patch {
        fn apply(&self, linux: &Path) -> Result<Keyboard, Incoherent> {
            self.prepared(linux).map(|(keyboard, _)| keyboard)
        }
        fn prepared(&self, linux: &Path) -> Result<(Keyboard, Pin), Incoherent> {
            self.prepare(linux, Self::admitted(linux)?)
        }
        fn admitted(linux: &Path) -> Result<Admitted, Incoherent> {
            Admitted::launch(linux.parent().unwrap().parent().unwrap())
        }
        fn with(&self, linux: &Path, faults: Faults) -> Result<Keyboard, Incoherent> {
            match self.prepare_with(linux, Self::admitted(linux)?, faults) {
                Ok(done) => done.map(|(keyboard, _)| keyboard),
                Err(Crashed) => panic!("crashed"),
            }
        }
        fn crash(&self, linux: &Path, faults: Faults) {
            assert!(self.prepare_with(linux, Self::admitted(linux).unwrap(), faults).is_err(), "{faults:?} was to crash");
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
            idle: |_| Ok(()),
            // What an earlier build of ours left whole: the earlier files everywhere, and the one with Voile's server.
            published: vec![
                vec![sha(b"our earlier server"); 3],
                vec![sha(b"our wsd"), sha(b"our driver"), sha(b"voile's server")],
            ],
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
        let release = root.path().join("Voile").join(version);
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

    #[cfg_attr(feature = "developer-timeline", ignore = "two of its files come from a folder (developer.rs)")]
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
        assert!(!linux.join(LEGACY_SESSIONS).exists() && !linux.join(LEGACY_WRITER).exists(), "the locks are elsewhere");
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
        // (until 0.1.8 a game started on it: no complete generation, no launch)
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [DRIVER]);
        assert_eq!(patch().keyboard(&linux), Keyboard::Us);
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
        let refused = patch().apply(&linux).unwrap_err();
        assert_eq!(refused.paths, [WSD]);
        assert!(refused.reason.contains("missing"), "{}", refused.reason);
        assert_eq!(refused.action, Action::Repair);
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
        fs::remove_file(root.path().join("Voile").join(RELEASE).join("runtime.json")).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
    }

    #[test]
    fn a_render_server_that_is_neither_voile_s_nor_ours_leaves_every_file_alone() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(SERVER), b"someone else's server").unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [SERVER]);
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
        assert_eq!(apply(linux).unwrap().0, Keyboard::Mac);
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
            let release = root.path().join("Voile").join(&patch.release);
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
            // What it left is a complete generation: a game starts on it while another one's lock keeps it as it is.
            let session = Pin::hold(&linux).unwrap();
            assert!(patch.apply(&linux).is_ok(), "{tag}");
            drop(session);
            assert_eq!(patch.apply(&linux).unwrap(), Keyboard::Mac, "{tag}");
            for file in &patch.files {
                assert_eq!(crate::sha(&linux.join(&file.path)).unwrap(), file.patched, "{tag} {}", file.path);
            }
            tried += 1;
        }
        assert!(tried >= 12, "the cli-v0.1.2 to 0.1.13 tags");
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
        let (keyboard, second) = patch().prepared(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Pending);
        drop(session);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending, "the second launch's pin still holds");
        drop(second);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert_eq!(one_set(&linux), "ours");
    }

    /// A preparation's pin lasts as long as what holds it: the launch that carries it.
    #[test]
    fn the_preparation_pin_is_held_until_released() {
        let (_root, linux) = runtime(RELEASE);
        let (keyboard, pin) = patch().prepared(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Mac);
        let sessions = Locks::of(&linux).unwrap().sessions().unwrap();
        assert!(matches!(sessions.try_lock(), Err(fs::TryLockError::WouldBlock)));
        drop(pin);
        free(&sessions);
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

    /// A lock another test's child process held for the moment before its exec is free soon.
    fn free(lock: &fs::File) {
        for _ in 0..200 {
            if lock.try_lock().is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the lock is still held");
    }

    fn set(linux: &Path, files: [&[u8]; 3]) {
        for (path, bytes) in [WSD, DRIVER, SERVER].into_iter().zip(files) {
            fs::write(linux.join(path), bytes).unwrap();
        }
    }

    fn bytes(linux: &Path) -> Vec<Vec<u8>> {
        [WSD, DRIVER, SERVER].iter().map(|path| fs::read(linux.join(path)).unwrap_or_default()).collect()
    }

    /// Fifth verification, finding 1 (the verifier's reproduction and its neighbours): a mixture that cannot be
    /// changed now (a full disk at any write, another session's lock, an unwritable runtime) starts no game;
    /// every combination of the files is either a complete released generation or refused.
    #[test]
    fn a_mixture_that_cannot_be_completed_starts_no_game() {
        let mixed: [&[u8]; 3] = [b"our wsd", b"voile's driver", b"voile's server"];
        for faults in [
            Faults { journal_write: Some(0), ..Faults::default() },
            Faults { journal_write: Some(1), ..Faults::default() },
            Faults { journal_write: Some(2), ..Faults::default() },
            Faults { stage: Some(0), ..Faults::default() },
            Faults { stage: Some(1), ..Faults::default() },
            Faults { switch: Some(0), ..Faults::default() },
            Faults { switch: Some(1), ..Faults::default() },
        ] {
            let (_root, linux) = runtime(RELEASE);
            set(&linux, mixed);
            let refused = patch().with(&linux, faults).unwrap_err();
            assert_eq!(refused.paths, [WSD], "{faults:?}");
            assert_eq!(refused.action, Action::Repair);
            assert_eq!(bytes(&linux), mixed, "{faults:?}");
            assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac, "then it is completed");
        }
        // Another session holds the runtime.
        let (_root, linux) = runtime(RELEASE);
        set(&linux, mixed);
        let session = Pin::hold(&linux).unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [WSD]);
        drop(session);
        // The folder cannot be written.
        let folder = linux.join(DRIVER).parent().unwrap().to_owned();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(patch().apply(&linux).is_err());
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(bytes(&linux), mixed);
        // An earlier session of 0.1.8 holds it through the lock it kept in the runtime.
        fs::write(linux.join(LEGACY_SESSIONS), b"").unwrap();
        let legacy = open_lock(&linux.join(LEGACY_SESSIONS)).unwrap();
        legacy.lock_shared().unwrap();
        assert!(patch().apply(&linux).is_err());
        drop(legacy);
        // Every combination of the four known files, under another session's lock.
        let choices: [[&[u8]; 4]; 3] = [
            [b"voile's wsd", b"our wsd", b"our earlier server", b"someone else's"],
            [b"voile's driver", b"our driver", b"our earlier server", b"someone else's"],
            [b"voile's server", b"our server", b"our earlier server", b"someone else's"],
        ];
        let _session = Pin::hold(&linux).unwrap();
        for (a, b, c) in (0..64).map(|n| (n % 4, n / 4 % 4, n / 16)) {
            set(&linux, [choices[0][a], choices[1][b], choices[2][c]]);
            let complete = (a == b && b == c && a < 3) || (a, b, c) == (1, 1, 0);
            assert_eq!(patch().apply(&linux).is_ok(), complete, "{a} {b} {c}");
        }
    }

    /// Finding 1: our files in a runtime that declares another release are not that release's.
    #[test]
    fn our_files_in_another_release_start_no_game() {
        let (_root, linux) = runtime("v1.0.0-2");
        fs::write(linux.join(DRIVER), b"our driver").unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [DRIVER]);
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
    }

    /// Finding 1: an engine file that is a link to one of ours may change under the game: not a file of the set.
    #[test]
    fn a_link_in_the_place_of_an_engine_file_starts_no_game() {
        let (root, linux) = runtime(RELEASE);
        let elsewhere = root.path().join("wsd");
        fs::write(&elsewhere, b"voile's wsd").unwrap();
        fs::remove_file(linux.join(WSD)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, linux.join(WSD)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [WSD]);
        assert_eq!(fs::read(&elsewhere).unwrap(), b"voile's wsd");
    }

    /// Finding 2: no pin, no launch.
    #[test]
    fn a_runtime_that_cannot_be_pinned_starts_no_game() {
        let (root, linux) = runtime(RELEASE);
        let locks = root.path().join("Voile.locks");
        // Its locks' folder cannot be made, and what stands there is no folder.
        fs::write(&locks, b"").unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        assert_eq!(refused.action, Action::CheckStorage);
        assert_eq!(one_set(&linux), "voile");
        fs::remove_file(&locks).unwrap();
        // A lock file that cannot be opened.
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        let path = Locks::of(&linux).unwrap().sessions_path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().action, Action::CheckStorage);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        // Another release is pinned too.
        let (root, linux) = runtime("v1.0.0-2");
        fs::write(root.path().join("Voile.locks"), b"").unwrap();
        assert!(patch().apply(&linux).is_err());
    }

    /// Finding 2: the lock outlives the runtime's folder, whatever replaces it, and the installer waits its turn.
    #[test]
    fn the_locks_are_outside_the_folder_an_install_replaces() {
        let (root, linux) = runtime(RELEASE);
        let (_, pin) = patch().prepared(&linux).unwrap();
        let release = root.path().join("Voile").join(RELEASE);
        assert!(Replacing::hold(&release).unwrap().is_none(), "a held runtime is not replaced");
        // Whatever took the folder away all the same, the lock is the same one.
        fs::rename(&release, root.path().join("Voile/moved")).unwrap();
        assert!(Replacing::hold(&release).unwrap().is_none());
        fs::rename(root.path().join("Voile/moved"), &release).unwrap();
        drop(pin);
        let mut held = None;
        for _ in 0..200 {
            held = Replacing::hold(&release).unwrap();
            if held.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // While it is replaced no launch prepares: it waits, then finds what is there.
        let held = held.unwrap();
        let waiting = std::thread::spawn({
            let linux = linux.clone();
            move || patch().apply(&linux)
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!waiting.is_finished());
        set(&linux, [b"voile's wsd", b"voile's driver", b"voile's server"]);
        drop(held);
        assert_eq!(waiting.join().unwrap().unwrap(), Keyboard::Mac);
        // A session of an older build, which locked inside the runtime, is not replaced either.
        let legacy = open_lock(&linux.join(LEGACY_SESSIONS)).unwrap();
        legacy.lock_shared().unwrap();
        assert!(Replacing::hold(&release).unwrap().is_none());
    }

    /// Finding 2: a supervisor's pin is the preparation's own lock, inherited: the preparation's going (a crash of
    /// the app between the two) never lets go of it, and only that runtime's held lock is taken for one.
    #[test]
    fn an_inherited_pin_is_the_preparation_s_own_lock() {
        let (_root, linux) = runtime(RELEASE);
        let (_, pin) = patch().prepared(&linux).unwrap();
        let inherited = Pin::adopt(&linux, pin.library.try_clone().unwrap()).unwrap();
        drop(pin);
        let sessions = Locks::of(&linux).unwrap().sessions().unwrap();
        assert!(matches!(sessions.try_lock(), Err(fs::TryLockError::WouldBlock)), "held still");
        drop(inherited);
        free(&sessions);
        drop(sessions);
        // Something else in its place is none: another file, another runtime's lock.
        let (_other, other) = runtime(RELEASE);
        let (_, foreign) = patch().prepared(&other).unwrap();
        assert!(Pin::adopt(&linux, foreign.library.try_clone().unwrap()).is_err());
        assert!(Pin::adopt(&linux, fs::File::open("/dev/null").unwrap()).is_err());
        // Nor is a lock nobody held while a writer has it.
        let writer = Locks::of(&linux).unwrap().sessions().unwrap();
        free(&writer);
        assert!(Pin::adopt(&linux, open_lock(&Locks::of(&linux).unwrap().sessions_path()).unwrap()).is_err());
    }

    /// Eighth verification, blocker: a lock file that is there and cannot be written is held all the same, by the
    /// launch, by its inheritor and against a change; only one that is missing is made, and nothing but a file is one.
    #[test]
    fn a_lock_file_that_cannot_be_written_is_held_all_the_same() {
        let (_root, linux) = runtime(RELEASE);
        let release = linux.parent().unwrap();
        drop(patch().prepared(&linux).unwrap());
        let library = library_lock(release.parent().unwrap()).unwrap();
        let folder = library.parent().unwrap();
        let inode = fs::metadata(&library).unwrap().ino();
        let locks: Vec<PathBuf> = fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().path()).collect();
        assert!(locks.len() >= 3, "{locks:?}");
        for lock in &locks {
            fs::set_permissions(lock, fs::Permissions::from_mode(0o444)).unwrap();
        }
        let (_, pin) = patch().prepared(&linux).unwrap();
        assert!(Replacing::free(release).unwrap().is_none(), "a change waits for it");
        let inherited = Pin::adopt(&linux, pin.library.try_clone().unwrap()).unwrap();
        drop(pin);
        assert!(Alone::hold(release.parent().unwrap()).unwrap().is_none(), "held still");
        drop(inherited);
        drop(Replacing::hold(release).unwrap().unwrap());
        assert_eq!(fs::metadata(&library).unwrap().ino(), inode, "the same file");
        assert_eq!(fs::metadata(&library).unwrap().permissions().mode() & 0o777, 0o444);
        // Made where it is missing; a folder or a link to nothing in its place is none.
        fs::remove_file(&library).unwrap();
        drop(patch().prepared(&linux).unwrap());
        assert!(library.is_file());
        fs::remove_file(&library).unwrap();
        fs::create_dir(&library).unwrap();
        assert_eq!(patch().prepared(&linux).unwrap_err().action, Action::CheckStorage);
        fs::remove_dir(&library).unwrap();
        std::os::unix::fs::symlink(folder.join("nothing"), &library).unwrap();
        assert_eq!(patch().prepared(&linux).unwrap_err().action, Action::CheckStorage);
    }

    /// Finding 3 (the verifier's reproduction and its neighbours): a record that parses and is not one this program
    /// can have written changes nothing, and stays with the kept originals.
    #[test]
    fn a_record_that_is_not_one_is_refused_and_kept() {
        let spoilt: [fn(&mut Journal); 10] = [
            |j| j.files.clear(),
            |j| {
                j.files.pop();
            },
            |j| {
                let twice = j.files[0].clone();
                j.files.push(twice);
            },
            |j| j.files[2] = j.files[0].clone(),
            |j| j.files[0].path = "../../../../elsewhere".to_owned(),
            |j| j.files.push(JournalFile { path: "/etc/hosts".to_owned(), before: sha(b"a"), after: sha(b"b") }),
            |j| j.id = "../../x".to_owned(),
            |j| j.id = String::new(),
            |j| j.files[1].before = sha(b"anything"),
            |j| j.files[1].after = sha(b"anything"),
        ];
        for phase in [Phase::Staging, Phase::Switching, Phase::Committed] {
            for (index, spoil) in spoilt.iter().enumerate() {
                let (_root, linux) = runtime(RELEASE);
                if phase == Phase::Committed {
                    set(&linux, [b"our wsd", b"our driver", b"our server"]);
                } else {
                    fs::write(linux.join(WSD), b"our wsd").unwrap();
                }
                let mut record = journal(&linux, phase, RELEASE);
                let old = txn_name(&linux.join(WSD), &record.id, "old");
                fs::write(&old, b"voile's wsd").unwrap();
                spoil(&mut record);
                record.write_to(&linux);
                let (files, written) = (bytes(&linux), fs::read(linux.join(JOURNAL)).unwrap());
                let refused = patch().apply(&linux).unwrap_err();
                assert_eq!(refused.paths, [JOURNAL], "{phase:?} {index}");
                assert_eq!(bytes(&linux), files, "{phase:?} {index}");
                assert_eq!(fs::read(linux.join(JOURNAL)).unwrap(), written);
                assert_eq!(fs::read(&old).unwrap(), b"voile's wsd");
            }
        }
        // A link, or a folder, where the record should be is not "no record".
        let (_root, linux) = runtime(RELEASE);
        std::os::unix::fs::symlink("nowhere", linux.join(JOURNAL)).unwrap();
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [JOURNAL]);
    }

    /// Finding 3: nothing is put back before everything that will be is known to be there.
    #[test]
    fn a_recovery_that_cannot_be_finished_puts_nothing_back() {
        let (_root, linux) = runtime(RELEASE);
        set(&linux, [b"our wsd", b"our driver", b"our server"]);
        let record = journal(&linux, Phase::Switching, RELEASE);
        for (path, kept) in [(WSD, "voile's wsd"), (DRIVER, "voile's driver")] {
            fs::write(txn_name(&linux.join(path), &record.id, "old"), kept).unwrap();
        }
        // (the third file's original is missing)
        assert_eq!(patch().apply(&linux).unwrap_err().paths, [SERVER]);
        assert_eq!(bytes(&linux), [b"our wsd".as_slice(), b"our driver", b"our server"]);
        assert!(linux.join(JOURNAL).exists());
        assert!(txn_name(&linux.join(WSD), &record.id, "old").exists() && txn_name(&linux.join(DRIVER), &record.id, "old").exists());
        // Present but someone else's: the same.
        fs::write(txn_name(&linux.join(SERVER), &record.id, "old"), b"someone else's").unwrap();
        assert!(patch().apply(&linux).is_err());
        assert_eq!(bytes(&linux), [b"our wsd".as_slice(), b"our driver", b"our server"]);
        fs::write(txn_name(&linux.join(SERVER), &record.id, "old"), b"voile's server").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        no_leftovers(&linux);
    }

    /// Finding 3: a folder that cannot be made durable is an error of the step it belongs to, wherever it comes:
    /// the launch starts on one set, or not at all, and the next one always ends it.
    #[test]
    fn a_sync_that_fails_is_never_passed_over() {
        let mut refused = 0;
        for left in 0..24 {
            for crash in [None, Some(1), Some(2)] {
                let (_root, linux) = runtime(RELEASE);
                if crash.is_some() {
                    patch().crash(&linux, Faults { crash_after: Some(Phase::Switching), crash_switched: crash, ..Faults::default() });
                }
                SYNCS_LEFT.set(Some(left));
                let done = patch().apply(&linux);
                SYNCS_LEFT.set(None);
                match done {
                    Ok(_) => drop(one_set(&linux)),
                    Err(_) => {
                        refused += 1;
                        assert!(linux.join(JOURNAL).exists(), "{left} {crash:?}: refused without a record");
                    }
                }
                // (another test's child may hold the locks for the moment before its exec)
                free(&open_lock(&library_lock(linux.parent().unwrap().parent().unwrap()).unwrap()).unwrap());
                free(&Locks::of(&linux).unwrap().sessions().unwrap());
                assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac, "{left} {crash:?}");
                assert_eq!(one_set(&linux), "ours");
                no_leftovers(&linux);
            }
        }
        assert!(refused > 0, "a recovery that is not durable is refused");
    }

    /// Finding 3: originals kept by a transaction whose record went (an older build said to delete it) stay while
    /// the set is not whole: they may be its only good copies.
    #[test]
    fn kept_originals_stay_while_the_set_is_not_whole() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"damaged").unwrap();
        let old = txn_name(&linux.join(WSD), "1a", "old");
        fs::write(&old, b"voile's wsd").unwrap();
        assert!(patch().apply(&linux).is_err());
        assert_eq!(fs::read(&old).unwrap(), b"voile's wsd");
        fs::write(linux.join(WSD), b"voile's wsd").unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert!(!old.exists());
    }

    /// Finding 5: a refusal crosses as a localization key with the reason, what ends it, and every path; nothing
    /// in it tells a player to delete a file.
    #[test]
    fn a_refusal_crosses_with_its_paths_and_what_ends_it() {
        let (_root, linux) = runtime(RELEASE);
        fs::remove_file(linux.join(WSD)).unwrap();
        fs::write(linux.join(SERVER), b"someone else's").unwrap();
        let refused = patch().apply(&linux).unwrap_err();
        let told: serde_json::Value = serde_json::from_str(&refused.message()).unwrap();
        assert_eq!(told["key"], Incoherent::KEY);
        assert_eq!(told["args"][1], "repair");
        assert_eq!(told["args"].as_array().unwrap()[2..], [WSD, SERVER]);
        assert!(!told["args"][0].as_str().unwrap().contains("remove"), "{told}");
        let error = crate::launch::Error::EngineFiles(std::sync::Arc::new(refused.clone()));
        assert_eq!(error.to_string(), refused.message());
        assert_eq!(crate::steam::Error::from(error).to_string(), refused.message());
    }

    /// What is built in: every published generation is one of known files, the shipped one among them.
    #[test]
    fn the_published_generations_are_of_known_files() {
        let manifest: Manifest = serde_json::from_str(MANIFEST).unwrap();
        let releases: Vec<&str> = manifest.generations.iter().flat_map(|g| g.releases.iter().map(String::as_str)).collect();
        for release in ["0.1.2", "0.1.3", "0.1.4", "0.1.5", "0.1.6", "0.1.7", "0.1.8", "0.1.9", "0.1.10", "0.1.11", "0.1.12", "0.1.13"] {
            assert!(releases.contains(&release), "{release}");
        }
        let patch = built_in();
        for generation in &manifest.generations {
            assert!(generation.files.keys().all(|path| patch.files.iter().any(|file| file.path == *path)));
        }
        for set in patch.generations() {
            assert!(patch.files.iter().zip(set).all(|(file, sha)| file.known(sha)));
        }
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

    /// Sixth verification, ruling 4 (the verifier's fixture): a record whose files `after` are no published set is
    /// not one this program can have written: nothing is deleted from it, and no launch starts.
    #[test]
    fn a_committed_mixture_keeps_its_verified_predecessor() {
        let (_root, linux) = runtime(RELEASE);
        let patch = patch();
        let mut record = journal(&linux, Phase::Committed, RELEASE);
        record.files[1].after = sha(b"our earlier server");
        for (index, file) in record.files.iter().enumerate() {
            let target = linux.join(&file.path);
            fs::copy(&target, txn_name(&target, &record.id, "old")).unwrap();
            let bytes = if index == 1 { &b"our earlier server"[..] } else { patch.files[index].bytes };
            fs::write(target, bytes).unwrap();
        }
        let record = record.write_to(&linux);
        assert!(patch.recover(&linux, &record).is_err());
        assert_eq!(patch.apply(&linux).unwrap_err().paths, [JOURNAL]);
        assert!(record.files.iter().all(|file| txn_name(&linux.join(&file.path), &record.id, "old").exists()));
        assert!(linux.join(JOURNAL).exists());
    }

    /// Ruling 4: an update undone to a set that is no published one (a mixture it was completing) keeps its record
    /// and its kept originals, until a later one is whole.
    #[test]
    fn an_update_undone_to_a_mixture_keeps_everything() {
        let (_root, linux) = runtime(RELEASE);
        set(&linux, [b"our wsd", b"voile's driver", b"voile's server"]);
        patch().crash(&linux, Faults { crash_after: Some(Phase::Switching), crash_switched: Some(1), ..Faults::default() });
        let kept = |path: &str| {
            let folder = linux.join(path).parent().unwrap().to_owned();
            fs::read_dir(folder).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().contains("-old-"))
        };
        assert!(kept(DRIVER) && kept(SERVER));
        // (a full disk: no new update can be recorded)
        let refused = patch().with(&linux, Faults { journal_write: Some(0), ..Faults::default() }).unwrap_err();
        assert_eq!((refused.paths, refused.action), (vec![WSD.to_owned()], Action::Repair));
        assert!(kept(DRIVER) && kept(SERVER) && linux.join(JOURNAL).exists());
        assert_eq!(bytes(&linux), [b"our wsd".as_slice(), b"voile's driver", b"voile's server"]);
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        no_leftovers(&linux);
    }

    /// Sixth verification, the legacy locks (the verifier's fixture): a lock of an older build that is there and
    /// held is a game's, whatever its permissions; one that cannot be opened is an error, never "absent".
    #[test]
    fn an_older_build_s_lock_is_never_taken_for_absent() {
        let (_root, linux) = runtime(RELEASE);
        let release = linux.parent().unwrap().to_owned();
        let path = linux.join(LEGACY_SESSIONS);
        let old = open_lock(&path).unwrap();
        old.lock_shared().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(Replacing::hold(&release).unwrap().is_none());
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending, "and no update under it");
        for path in [LEGACY_SESSIONS, LEGACY_WRITER] {
            let path = linux.join(path);
            drop(open_lock(&path));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            assert!(Replacing::hold(&release).is_err());
            assert_eq!(patch().apply(&linux).unwrap_err().action, Action::CheckStorage);
            assert_eq!(one_set(&linux), "voile");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        drop(old);
        let mut free = None;
        for _ in 0..200 {
            free = Replacing::hold(&release).unwrap();
            if free.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(free.is_some());
    }

    /// The same finding, the other way: where an older build would find files to change under a pinned game, its own
    /// lock is made and held with the pin. (Ours in place, it finds none, and none is made.)
    #[test]
    fn an_older_build_cannot_write_under_a_pin_it_does_not_know() {
        let (_root, linux) = runtime(RELEASE);
        let session = Pin::hold(&linux).unwrap();
        let (keyboard, pin) = patch().prepared(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Pending);
        let legacy = open_lock(&linux.join(LEGACY_SESSIONS)).unwrap();
        assert!(!alone(&legacy).unwrap());
        // The supervisor holds it too.
        let inherited = Pin::adopt(&linux, pin.library.try_clone().unwrap()).unwrap();
        drop((pin, session));
        assert!(!alone(&legacy).unwrap());
        drop(inherited);
        free(&legacy);
        legacy.unlock().unwrap();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
    }

    /// Ruling 5: the locks' folder comes with the library, and never needs a library that can be written again.
    #[test]
    fn a_provisioned_library_never_needs_to_be_written_for_its_locks() {
        for voile_there in [false, true] {
            let library = tempfile::tempdir().unwrap();
            if voile_there {
                fs::create_dir(library.path().join("Voile")).unwrap();
            }
            provision(library.path()).unwrap();
            provision(library.path()).unwrap();
            assert!(library.path().join("Voile.locks").is_dir());
            let release = library.path().join("Voile").join(RELEASE);
            let linux = release.join("linux");
            for (path, bytes) in [(WSD, "voile's wsd"), (DRIVER, "voile's driver"), (SERVER, "voile's server")] {
                fs::create_dir_all(linux.join(path).parent().unwrap()).unwrap();
                fs::write(linux.join(path), bytes).unwrap();
            }
            fs::write(release.join("runtime.json"), format!(r#"{{"version":"{RELEASE}"}}"#)).unwrap();
            fs::set_permissions(library.path(), fs::Permissions::from_mode(0o555)).unwrap();
            let done = patch().apply(&linux);
            fs::set_permissions(library.path(), fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(done.unwrap(), Keyboard::Mac);
        }
        // Where the runtimes are a link, the locks are beside what it leads to.
        let (library, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::create_dir(disk.path().join("Voile")).unwrap();
        std::os::unix::fs::symlink(disk.path().join("Voile"), library.path().join("Voile")).unwrap();
        provision(library.path()).unwrap();
        assert!(disk.path().join("Voile.locks").is_dir());
    }

    /// Sixth verification: a complete set with only an update's cleanup pending starts while another game runs, and
    /// the cleanup waits for it.
    #[test]
    fn a_complete_set_with_only_cleanup_pending_starts() {
        let (_root, linux) = runtime(RELEASE);
        for crash in [Phase::Committed, Phase::Staging] {
            patch().crash(&linux, Faults { crash_after: Some(crash), ..Faults::default() });
            let expected = if crash == Phase::Committed { Keyboard::Mac } else { Keyboard::Pending };
            // The verifier's: an older build's session holds its lock.
            fs::write(linux.join(LEGACY_SESSIONS), b"").unwrap();
            let legacy = open_lock(&linux.join(LEGACY_SESSIONS)).unwrap();
            legacy.lock_shared().unwrap();
            assert_eq!(patch().apply(&linux).unwrap(), expected, "{crash:?}");
            assert!(linux.join(JOURNAL).exists(), "{crash:?}: the cleanup waits");
            drop(legacy);
            // This build's.
            let session = Pin::hold(&linux).unwrap();
            assert_eq!(patch().apply(&linux).unwrap(), expected, "{crash:?}");
            drop(session);
            // A game no lock shows.
            let running = Patch { idle: |_| Err("a game runs".into()), ..patch() };
            assert_eq!(running.apply(&linux).unwrap(), expected, "{crash:?}");
            assert!(linux.join(JOURNAL).exists());
            assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
            no_leftovers(&linux);
            set(&linux, [b"voile's wsd", b"voile's driver", b"voile's server"]);
        }
        // Half switched it is not complete: refused, with the same answer, and nothing changes.
        patch().crash(&linux, Faults { crash_after: Some(Phase::Switching), crash_switched: Some(1), ..Faults::default() });
        let before = bytes(&linux);
        let running = Patch { idle: |_| Err("a game runs".into()), ..patch() };
        assert_eq!(running.apply(&linux).unwrap_err().action, Action::QuitGame);
        assert_eq!(bytes(&linux), before);
    }

    /// Ruling 2: whatever the locks say, no file changes while something of the library's engine runs, or when that
    /// cannot be told; a launch on files that need nothing does not look.
    #[test]
    fn no_file_changes_while_something_of_the_engine_runs() {
        let (_root, linux) = runtime(RELEASE);
        let running = || Patch { idle: |_| Err("a game runs".into()), ..patch() };
        assert_eq!(running().apply(&linux).unwrap(), Keyboard::Pending);
        assert_eq!(one_set(&linux), "voile");
        // A leftover, and a sole copy kept aside, stay where they are too.
        fs::write(aside_of(&linux.join(WSD)), b"voile's wsd").unwrap();
        fs::remove_file(linux.join(WSD)).unwrap();
        assert!(running().apply(&linux).is_err());
        assert!(aside_of(&linux.join(WSD)).exists() && !linux.join(WSD).exists());
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        fs::write(aside_of(&linux.join(WSD)), b"voile's wsd").unwrap();
        assert_eq!(running().apply(&linux).unwrap(), Keyboard::Mac);
        assert!(aside_of(&linux.join(WSD)).exists());
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
        assert!(!aside_of(&linux.join(WSD)).exists());
        let never = Patch { idle: |_| panic!("looked"), ..patch() };
        assert_eq!(never.apply(&linux).unwrap(), Keyboard::Mac);
    }

    /// Ruling 2, with the real look: a program of the library's engine, no lock held, no record.
    #[test]
    fn a_program_of_the_engine_keeps_its_files_as_they_are() {
        use super::super::live::tests::Running;
        let (_root, linux) = runtime(RELEASE);
        let patch = || Patch { idle: super::super::live::idle, ..patch() };
        let running = Running::at(&linux.join("graphics/bin/virgl_test_server"));
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Pending);
        assert_eq!(one_set(&linux), "voile");
        running.end();
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
    }

    /// Seventh verification, blocker 2: one lock for the whole library. A launch admitted to it, on either engine and
    /// whichever release, holds every change off from before it picks a runtime to its game's exit; a change holds every
    /// launch off from before its look to its last check; and two changes are never made at once.
    #[test]
    fn a_launch_anywhere_in_the_library_holds_every_change_of_it_off() {
        let (root, linux) = runtime(RELEASE);
        let voile = root.path().join("Voile");
        let (release, other) = (voile.join(RELEASE), voile.join("v1.0.0-9"));
        let lock = || open_lock(&library_lock(&voile).unwrap()).unwrap();
        // Admitted, with no runtime picked yet; then pinned without one, as Rosetta's launches are.
        let admitted = Admitted::take(&voile).unwrap();
        for held in [&release, &other] {
            assert!(Replacing::hold(held).unwrap().is_none());
            assert!(Replacing::free(held).unwrap().is_none());
        }
        assert!(Alone::hold(&voile).unwrap().is_none());
        let pin = admitted.pin().unwrap();
        assert!(Replacing::hold(&release).unwrap().is_none() && Replacing::free(&other).unwrap().is_none());
        // Its supervisor is given that very lock, for Rosetta and for the Linux engine.
        let inherited = Pin::adopt_library(&voile, pin.library.try_clone().unwrap()).unwrap();
        // Another launch goes beside it, and changes nothing.
        let (keyboard, second) = patch().prepared(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Pending);
        assert_eq!(one_set(&linux), "voile");
        drop((pin, second));
        assert!(Alone::hold(&voile).unwrap().is_none(), "the supervisor's hold is the launch's");
        drop(inherited);
        free(&lock());
        // A lock that is not held, another library's, or no lock, is no pin.
        let writer = lock();
        free(&writer);
        assert!(Pin::adopt_library(&voile, lock()).is_err(), "someone changes the runtimes");
        drop(writer);
        let (elsewhere, _) = runtime(RELEASE);
        let foreign = Admitted::take(&elsewhere.path().join("Voile")).unwrap().pin().unwrap();
        assert!(Pin::adopt_library(&voile, foreign.library.try_clone().unwrap()).is_err());
        assert!(Pin::adopt_library(&voile, fs::File::open("/dev/null").unwrap()).is_err());
        // A change of one release holds off the change of another, and every launch, until it is done.
        free(&lock());
        let change = Replacing::free(&other).unwrap().unwrap();
        assert!(Replacing::hold(&release).unwrap().is_none());
        let waiting = std::thread::spawn({
            let linux = linux.clone();
            move || patch().apply(&linux)
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!waiting.is_finished());
        assert_eq!(one_set(&linux), "voile");
        drop(change);
        assert_eq!(waiting.join().unwrap().unwrap(), Keyboard::Mac);
        // The lock is outside whatever is replaced, beside the locks of every release.
        assert_eq!(library_lock(&voile).unwrap().parent().unwrap().file_name().unwrap(), "Voile.locks");
    }

    /// Blocker 2: the lock goes from the update to the launch it was made for with no moment between, and an update
    /// made by one launch is never under another.
    #[test]
    fn an_updated_library_is_held_by_the_launch_it_was_updated_for() {
        let (root, linux) = runtime(RELEASE);
        let voile = root.path().join("Voile");
        let (keyboard, pin) = patch().prepared(&linux).unwrap();
        assert_eq!(keyboard, Keyboard::Mac);
        assert!(Alone::hold(&voile).unwrap().is_none());
        let beside = Admitted::take(&voile).unwrap();
        assert!(!beside.alone, "and shared: another launch goes beside it");
        drop(beside);
        drop(pin);
        free(&open_lock(&library_lock(&voile).unwrap()).unwrap());
    }
}
