//! Voile, the Wine runtime Windows games run in. Obelisk runs the newest
//! release installed that keeps its contract: the paths, variables and
//! programs it relies on in a runtime, which docs/CONTRACT.md lists. As a
//! Windows game launches, Obelisk reads Voile's feed: the player takes a
//! newer release of its contract before the game starts, with what it brings
//! in view, or starts the game in the release in place unless it needs the
//! newer one. A Windows build's Transfer gets a release before the game's
//! files when none is installed. The release built into Obelisk stands in for
//! the feed until one was read. A player who takes beta releases runs Voile's
//! pre-releases too, whichever is newest; one who does not leaves a
//! pre-release in place for the newest release, as Obelisk leaves a
//! withdrawn release. A development runtime that voile's install-dev.sh put
//! in `current` takes precedence.
//!
//! The release also carries the Linux engine's runtime, in its `linux` folder.
use crate::runtimes::{Standing, Update};
use obelisk_core::transfer::CancelToken;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub mod engine_patch;
mod feed;
pub(crate) mod live;
mod members;
pub(crate) mod required;

use feed::Offered;
use members::Members;
pub(crate) use required::lacking;
pub(crate) use feed::refresh;
pub use feed::{take_prereleases, takes_prereleases};

const RELEASE: &str = include_str!("../voile-release.json");
/// The runtimes' folder, in the library.
pub(crate) const ROOT: &str = "Voile";
/// Where install-dev.sh puts a locally built runtime.
const DEVELOPMENT: &str = "current";
/// What every release archive describes itself with.
const MANIFEST: &str = "runtime.json";
/// Unfinished work in the runtimes folder starts with this, and the next
/// install removes it.
const UNFINISHED: &str = ".";
/// The Linux engine's runtime, inside a release (or the development runtime).
const LINUX: &str = "linux";

/// Two installs never extract into the same folder.
static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A Voile release: its version and its archive.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// `voile-release.json`: the contract Obelisk keeps with Voile, and the
/// release Obelisk was built with, of that contract.
#[derive(Debug, Deserialize)]
struct BuiltIn {
    contract: u32,
    #[serde(flatten)]
    release: Release,
}

fn built_in() -> &'static BuiltIn {
    static VALUE: OnceLock<BuiltIn> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(RELEASE).expect("checked-in Voile release"))
}

/// The release Obelisk was built with, which Transfers and launches install
/// until they read a feed.
pub fn release() -> &'static Release {
    &built_in().release
}

/// What Obelisk relies on in a runtime, as Voile's `CONTRACT` counts it:
/// Obelisk runs only releases of this contract.
pub fn contract() -> u32 {
    built_in().contract
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Failure {
    #[error("backend.voile.downloadFailed")]
    Download,
    #[error("backend.voile.damaged")]
    Damaged,
    #[error("backend.voile.storageFailed")]
    Storage,
    #[error("backend.voile.cancelled")]
    Cancelled,
    /// A game runs on the runtime that would be replaced.
    #[error("backend.voile.inUse")]
    InUse,
    /// The disk cannot exchange two folders in one step, as APFS does: what
    /// is at this path is never replaced there.
    #[error("{}", exchange_message(.0))]
    Exchange(PathBuf),
}

fn exchange_message(path: &Path) -> String {
    serde_json::json!({ "key": "backend.voile.noExchange", "args": [path.to_string_lossy()] }).to_string()
}
impl From<std::io::Error> for Failure {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}

/// The runtime in `root` Windows games run in: the development runtime, else
/// the newest release installed there that Obelisk runs.
pub fn runtime(root: &Path) -> Option<PathBuf> {
    if let Some(development) = development(root) {
        return Some(development);
    }
    newest(root, &Offer::read(root)).map(|(_, runtime)| runtime)
}

/// The runtime install-dev.sh put in `root`, when there is one.
fn development(root: &Path) -> Option<PathBuf> {
    let development = root.join(DEVELOPMENT);
    has_wine(&development).then_some(development)
}

/// The Wine of a runtime.
const WINE: &str = "bin/wine";

fn has_wine(runtime: &Path) -> bool {
    runtime.join(WINE).is_file()
}

/// Whether `runtime` is a release's folder, named as its runtime.json names
/// it, whatever it lost: the development runtime is none.
fn is_release(runtime: &Path) -> bool {
    let name = runtime.file_name().and_then(|name| name.to_str());
    name.is_some_and(|name| numbers(name).is_some() && version(runtime).as_deref() == Some(name))
}

/// Whether `runtime` holds the Wine of `release`, as its runtime.json names
/// it.
fn holds(runtime: &Path, release: &str) -> bool {
    has_wine(runtime) && version(runtime).as_deref() == Some(release)
}

/// The Linux engine's runtime in `root`: the development runtime's, else the
/// one inside the release Windows games run in, as that release has it: what
/// it lost is for whoever starts a game there to tell ([`missing`]).
pub fn linux_runtime(root: &Path) -> Option<PathBuf> {
    let development = root.join(DEVELOPMENT).join(LINUX);
    if has_engine(&development) {
        return Some(development);
    }
    let release = newest(root, &Offer::read(root))?.1;
    let linux = release.join(LINUX);
    // A release whose files are ours to patch carries an engine, whatever is
    // left of it; another carries one when its folder is there.
    let carries = version(&release).as_deref() == Some(engine_patch::release());
    (carries || linux.is_dir()).then_some(linux)
}

/// Whether the runtime Windows games run in is the one install-dev.sh put
/// there, which no release replaces.
pub fn is_development(root: &Path) -> bool {
    development(root).is_some()
}

/// What each of this build's engine files is in the Linux engine of the Voile in `root`: `voile`, `ours` or `other`.
pub fn engine_files(root: &Path) -> Option<Vec<(String, &'static str)>> {
    linux_runtime(root).map(|linux| engine_patch::components(&linux))
}

/// Which keyboard the games of the Linux engine in `root` get: the layout
/// selected on the Mac once the Orbiters fork's engine files are in place,
/// which they are from the first launch on the engine of the release they
/// are for; a US keyboard in any other release. None without a Linux engine.
pub fn keyboard(root: &Path) -> Option<engine_patch::Keyboard> {
    linux_runtime(root).map(|linux| engine_patch::keyboard(&linux))
}

/// The engine and the Wine it runs.
const ENGINE: [&str; 2] = ["engine/fexdarwin-run", "wine/bin/wine"];

fn has_engine(runtime: &Path) -> bool {
    runtime.join(ENGINE[0]).is_file() && runtime.join(ENGINE[1]).exists()
}

/// The Voile release Windows games run in now, as its runtime.json names it.
pub fn installed_release(root: &Path) -> Option<String> {
    release_of(&runtime(root)?)
}

/// The Voile release `runtime` comes from: a Rosetta runtime, or the Linux
/// engine's runtime inside one.
pub(crate) fn release_of(runtime: &Path) -> Option<String> {
    version(runtime).or_else(|| version(runtime.parent()?))
}

/// What a release says of itself in its runtime.json.
#[derive(Deserialize)]
struct Manifest {
    version: String,
    /// The releases before `CONTRACT` kept the first contract.
    #[serde(default = "first_contract")]
    contract: u32,
}

fn first_contract() -> u32 {
    1
}

fn manifest(runtime: &Path) -> Option<Manifest> {
    serde_json::from_slice(&fs::read(runtime.join(MANIFEST)).ok()?).ok()
}

fn version(runtime: &Path) -> Option<String> {
    manifest(runtime).map(|manifest| manifest.version)
}

/// The numbers that order a release's version, as Voile's tags write it:
/// `v<CrossOver version>-<revision>`, such as `v26.3.0-16`, which is
/// [26, 3, 0, 16, 1, 0]. Its betas come before it: `v26.3.0-16.beta2` is
/// [26, 3, 0, 16, 0, 2]. Anything else names no release.
fn numbers(version: &str) -> Option<Vec<u64>> {
    let version = version.strip_prefix('v')?;
    let (release, stage) = match version.split_once(".beta") {
        Some((release, beta)) => (release, [0, digits(beta)?]),
        None => (version, [1, 0]),
    };
    let (crossover, revision) = release.split_once('-')?;
    let mut numbers = crossover
        .split('.')
        .map(digits)
        .collect::<Option<Vec<_>>>()?;
    if numbers.len() != 3 {
        return None;
    }
    numbers.push(digits(revision)?);
    numbers.extend(stage);
    Some(numbers)
}

/// The number `text` writes in digits alone: no sign, unlike `str::parse`.
fn digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Whether `version` names a release, not a beta, as Voile's tags do, such
/// as `v26.3.0-16`. Profiles name releases only: an Obelisk older than betas
/// leaves out an entry whose `minimum_voile` it cannot read.
pub(crate) fn is_version(version: &str) -> bool {
    numbers(version).is_some_and(|numbers| numbers[4] == 1)
}

/// Whether `version` names a beta, such as `v26.3.0-16.beta2`: a pre-release
/// even where the feed no longer lists it.
fn is_beta(version: &str) -> bool {
    numbers(version).is_some_and(|numbers| numbers[4] == 0)
}

/// The newest release installed in `root` that Obelisk runs, with its
/// version's numbers: a folder named after the release its runtime.json
/// names, of Obelisk's contract, that `offer` does not leave. Whatever it lost,
/// its Wine too, is for whoever starts a game there to tell ([`lacking`]).
fn newest(root: &Path, offer: &Offer) -> Option<(Vec<u64>, PathBuf)> {
    fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let order = numbers(&name)?;
            let runtime = entry.path();
            let manifest = manifest(&runtime)?;
            (manifest.version == name && manifest.contract == contract() && !offer.leaves(&name))
                .then_some((order, runtime))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
}

/// What the last feed read in a runtimes' folder offers the player, as they
/// chose to take beta releases or not.
struct Offer {
    offered: Vec<Offered>,
    /// The player takes Voile's pre-releases too.
    prereleases: bool,
}

impl Offer {
    fn read(root: &Path) -> Self {
        Self {
            offered: feed::offered(root),
            prereleases: feed::takes_prereleases(root),
        }
    }

    /// Whether Obelisk leaves `offered` for the newest release left: it is
    /// withdrawn, or a pre-release while the player takes none.
    fn left(&self, offered: &Offered) -> bool {
        offered.withdrawn.is_some() || (offered.prerelease && !self.prereleases)
    }

    /// Whether Obelisk leaves the release named `version`, as `left` tells.
    /// A release the feed does not name stays, unless it is a beta while the
    /// player takes none: the feed keeps no beta older than its newest
    /// release.
    fn leaves(&self, version: &str) -> bool {
        match self
            .offered
            .iter()
            .find(|offered| offered.release.version == version)
        {
            Some(offered) => self.left(offered),
            None => !self.prereleases && is_beta(version),
        }
    }

    /// The releases Obelisk runs, with their version's numbers: of its
    /// contract, and not left.
    fn runnable(&self) -> impl Iterator<Item = (Vec<u64>, &Offered)> {
        self.offered
            .iter()
            .filter(|offered| offered.contract == contract() && !self.left(offered))
            .filter_map(|offered| Some((numbers(&offered.release.version)?, offered)))
    }

    /// The release Obelisk wants: the newest it runs, else the one it was
    /// built with.
    fn wanted(&self) -> Release {
        self.runnable()
            .max_by(|a, b| a.0.cmp(&b.0))
            .map_or_else(|| release().clone(), |(_, newest)| newest.release.clone())
    }
}

/// What a launch makes of the runtimes in `root`, for a game whose profile
/// needs `minimum` or a newer release. A newer release comes with what each
/// release newer than the one in place brings, up to it, newest first.
pub(crate) fn standing(root: &Path, minimum: Option<&str>) -> Standing {
    if development(root).is_some() {
        return Standing::Current;
    }
    let offer = Offer::read(root);
    let Some((installed, _)) = newest(root, &offer) else {
        return Standing::Missing;
    };
    let version = offer.wanted().version;
    let Some(wanted) = numbers(&version) else {
        return Standing::Current;
    };
    if installed >= wanted {
        return Standing::Current;
    }
    // A game that needs a newer release cannot skip the one wanted, when that
    // one is enough.
    let required = minimum
        .and_then(numbers)
        .is_some_and(|minimum| installed < minimum && wanted >= minimum);
    let mut newer: Vec<_> = offer
        .runnable()
        .filter(|(order, _)| *order > installed && *order <= wanted)
        .collect();
    newer.sort_by(|a, b| b.0.cmp(&a.0));
    let releases = newer
        .into_iter()
        .map(|(_, offered)| Notes {
            version: offered.release.version.clone(),
            notes: offered.notes.clone(),
        })
        .collect();
    Standing::Outdated(Update {
        version,
        required,
        releases,
    })
}

/// What a Voile release brings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notes {
    pub version: String,
    /// In Markdown, as its GitHub release tells it; empty when the feed has
    /// none for it.
    pub notes: String,
}

/// One install of a release, which the player can cancel and watch.
#[derive(Default)]
pub struct Install {
    received: AtomicU64,
    /// The size of the archive being installed, once known.
    total: AtomicU64,
    cancel: CancelToken,
}

impl Install {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// The bytes downloaded so far, and the archive's size.
    pub fn progress(&self) -> (u64, u64) {
        let total = self.total.load(Ordering::Acquire);
        let total = if total == 0 { release().size } else { total };
        (self.received.load(Ordering::Acquire), total)
    }

    /// Installs in `root` the release Obelisk wants there once it read
    /// Voile's feed again, unless it or a development runtime is already
    /// there. It keeps the releases it replaces: a game may run from one, and
    /// `runtimes` decides when they go. `archive`, a local copy of the
    /// release Obelisk was built with, stands in for the feed and the
    /// download.
    pub async fn run(&self, root: &Path, archive: Option<&Path>) -> Result<PathBuf, Failure> {
        let release = match archive {
            Some(_) => release().clone(),
            None => {
                self.cancellable(feed::refresh(root)).await?;
                Offer::read(root).wanted()
            }
        };
        self.install(root, &release, archive).await
    }

    /// Installs again in `root` the release Windows games run in there, over
    /// what is in place: its files, its Linux engine's and whatever an engine
    /// update left. `archive` as in `run`. What is in place stays until the
    /// whole release is there, checked; a game that runs in it stops this
    /// (`InUse`). The development runtime is not a release: it is left.
    pub async fn repair(&self, root: &Path, archive: Option<&Path>) -> Result<PathBuf, Failure> {
        if archive.is_none() {
            self.cancellable(feed::refresh(root)).await?;
        }
        let offer = Offer::read(root);
        // The one in place, as far as what is left of it tells; else the one
        // an install would take.
        let release = match to_repair(root, &offer) {
            Some(version) if version == release().version => release().clone(),
            Some(version) => offer
                .offered
                .iter()
                .find(|offered| offered.release.version == version)
                .map_or_else(|| offer.wanted(), |offered| offered.release.clone()),
            None => offer.wanted(),
        };
        self.put(root, &release, archive, true).await
    }

    async fn install(
        &self,
        root: &Path,
        release: &Release,
        archive: Option<&Path>,
    ) -> Result<PathBuf, Failure> {
        self.put(root, release, archive, false).await
    }

    /// Puts `release` in place in `root`; over one in place, whole or not,
    /// when `again`.
    async fn put(
        &self,
        root: &Path,
        release: &Release,
        archive: Option<&Path>,
        again: bool,
    ) -> Result<PathBuf, Failure> {
        let _installing = self.cancellable(INSTALLING.lock()).await?;
        if let Some(development) = development(root) {
            return Ok(development);
        }
        let destination = root.join(&release.version);
        if holds(&destination, &release.version) && !again {
            return Ok(destination);
        }
        // One process at a time installs into a library.
        let alone = self.cancellable(Self::alone(root)).await??;
        if holds(&destination, &release.version) && !again {
            return Ok(destination);
        }
        // Found before the download, checked again as it goes in place.
        let replaces = fs::symlink_metadata(&destination).is_ok();
        let (runtime, folder) = (destination.clone(), root.to_owned());
        blocking(move || {
            if replaces {
                engine_patch::Replacing::hold(&runtime)?.ok_or(Failure::InUse)?;
            }
            // Nothing is laid on a disk no repair could be made on.
            exchanges(&folder)
        })
        .await?;
        step("before-extract");
        let (extracted, members) = self.extract(root, release, archive).await?;
        step("after-extract");
        let (root, release) = (root.to_owned(), release.version.clone());
        blocking(move || {
            // Held to the end, whoever waits for this.
            let _alone = alone;
            let runtime = extracted.path().join(format!("voile-{release}"));
            // The complete release, as its archive holds it to the byte, or
            // nothing is replaced: security software may have taken a file
            // meanwhile, a disk may have cut one short.
            whole(&runtime, &release, &members)?;
            step("after-validation");
            let held = if replaces {
                let held = engine_patch::Replacing::hold(&destination)?.ok_or(Failure::InUse)?;
                step("after-lock");
                exchange(&runtime, &destination)?;
                // Now the one it replaced: it goes only once the new one is
                // whole in its place too, else it takes its place back.
                let replaced = extracted.keep();
                step("after-exchange");
                if let Err(failure) = whole(&destination, &release, &members) {
                    exchange(&runtime, &destination)?;
                    let _ = fs::remove_dir_all(replaced);
                    return Err(failure);
                }
                step("after-verification");
                let _ = fs::remove_dir_all(replaced);
                held
            } else {
                use rustix::fs::{CWD, RenameFlags, renameat_with};
                let held = engine_patch::Replacing::free(&destination)?.ok_or(Failure::InUse)?;
                renameat_with(CWD, &runtime, CWD, &destination, RenameFlags::NOREPLACE)
                    .map_err(std::io::Error::from)?;
                if let Err(failure) = whole(&destination, &release, &members) {
                    renameat_with(CWD, &destination, CWD, &runtime, RenameFlags::NOREPLACE)
                        .map_err(std::io::Error::from)?;
                    return Err(failure);
                }
                held
            };
            // Nothing an unfinished install of it left is needed any more; a
            // whole runtime of another release kept there may be, and what
            // runs from one goes with a later install.
            if held.idle() {
                remove_entries(&root, |name| name.starts_with(UNFINISHED) && !saves_another(&root.join(name), &release));
            }
            step("after-cleanup");
            Ok(destination)
        })
        .await
    }

    /// Waits until no other process installs into `root`.
    async fn alone(root: &Path) -> Result<fs::File, Failure> {
        fs::create_dir_all(root)?;
        loop {
            if let Some(held) = engine_patch::installing(root)? {
                return Ok(held);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Checks `archive` (or downloads it) and extracts it into a folder of
    /// `root` that goes away unless the caller moves what it holds; returns
    /// it with what the archive holds, member by member. What an unfinished
    /// install left stays: it may hold the only runtime there is.
    async fn extract(
        &self,
        root: &Path,
        expected: &Release,
        archive: Option<&Path>,
    ) -> Result<(tempfile::TempDir, Members), Failure> {
        self.total.store(expected.size, Ordering::Release);
        fs::create_dir_all(root)?;
        // An unfinished download is no runtime.
        remove_entries(root, |name| {
            name.starts_with(UNFINISHED) && root.join(name).is_file()
        });
        let downloaded;
        let archive = match archive {
            Some(archive) => {
                let (path, expected_sha) = (archive.to_owned(), expected.sha256.clone());
                let matches = tokio::task::spawn_blocking(move || {
                    crate::sha(&path).is_ok_and(|sha| sha == expected_sha)
                })
                .await
                .map_err(|_| Failure::Storage)?;
                if !matches {
                    return Err(Failure::Damaged);
                }
                self.received.store(expected.size, Ordering::Release);
                archive
            }
            None => {
                downloaded = self.download(root, expected).await?;
                &*downloaded
            }
        };
        let extracted = tempfile::Builder::new()
            .prefix(UNFINISHED)
            .tempdir_in(root)?;
        let mut tar = tokio::process::Command::new("/usr/bin/tar")
            .arg("-xJf")
            .arg(archive)
            .arg("-C")
            .arg(extracted.path())
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        // What it holds is read from the archive itself, never from what was
        // laid of it.
        let mut listing = Members::listing(archive).spawn()?;
        let mut told = listing.stdout.take().ok_or(Failure::Storage)?;
        let mut members = Vec::new();
        let done = async {
            tokio::try_join!(
                tar.wait(),
                async {
                    tokio::io::AsyncReadExt::read_to_end(&mut told, &mut members).await?;
                    listing.wait().await
                },
            )
        };
        let (laid, listed) = self.cancellable(done).await??;
        if !laid.success() || !listed.success() {
            return Err(Failure::Damaged);
        }
        step("after-untar");
        let members = Members::read(&members, &format!("voile-{}", expected.version));
        Ok((extracted, members.ok_or(Failure::Damaged)?))
    }

    async fn download(
        &self,
        root: &Path,
        release: &Release,
    ) -> Result<tempfile::TempPath, Failure> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| Failure::Download)?;
        let mut response = self
            .cancellable(client.get(&release.url).send())
            .await?
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| Failure::Download)?;
        let mut file = tempfile::Builder::new()
            .prefix(UNFINISHED)
            .tempfile_in(root)?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        while let Some(bytes) = self
            .cancellable(response.chunk())
            .await?
            .map_err(|_| Failure::Download)?
        {
            size += bytes.len() as u64;
            if size > release.size {
                return Err(Failure::Damaged);
            }
            digest.update(&bytes);
            file.write_all(&bytes)?;
            self.received.store(size, Ordering::Release);
        }
        if size != release.size || format!("{:x}", digest.finalize()) != release.sha256 {
            return Err(Failure::Damaged);
        }
        Ok(file.into_temp_path())
    }

    /// Stops waiting for `work` as soon as the install is cancelled.
    async fn cancellable<T>(&self, work: impl Future<Output = T>) -> Result<T, Failure> {
        self.cancel.race(work).await.map_err(|_| Failure::Cancelled)
    }
}

/// Where a test may stop an install.
#[cfg(not(test))]
fn step(_name: &str) {}
#[cfg(test)]
use tests::step;

/// Runs `work`, which waits on locks or files, off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(work).await.map_err(|_| Failure::Storage)?
}

/// The release a repair of `root` installs again: the one Windows games run in
/// there, else the newest folder named after a release, whatever is missing
/// in it.
fn to_repair(root: &Path, offer: &Offer) -> Option<String> {
    let named = |path: PathBuf| Some(path.file_name()?.to_string_lossy().into_owned());
    if let Some((_, runtime)) = newest(root, offer) {
        return named(runtime);
    }
    fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let order = numbers(&name)?;
            (!offer.leaves(&name)).then_some((order, name))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, name)| name)
}

/// Checks that `runtime` is the complete `release`: its Wine, every member of
/// its archive as the archive holds it, and for the release our engine files
/// are for, its Linux engine as Voile ships it, with whatever a game of any
/// backend starts from.
fn whole(runtime: &Path, release: &str, members: &Members) -> Result<(), Failure> {
    if let Some(member) = members.differing(runtime) {
        eprintln!("engine: {} is not what its archive holds", runtime.join(member).display());
        return Err(Failure::Damaged);
    }
    let linux = runtime.join(LINUX);
    let complete = holds(runtime, release)
        && (release != engine_patch::release() || (has_engine(&linux) && required::complete(runtime)))
        && engine_patch::pristine(&linux, release);
    complete.then_some(()).ok_or(Failure::Damaged)
}

/// Whether `leftover`, of an install cut short, holds a whole runtime of
/// another release than `release`: it may be the only one there is.
fn saves_another(leftover: &Path, release: &str) -> bool {
    fs::read_dir(leftover).into_iter().flatten().flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.strip_prefix("voile-").is_some_and(|other| other != release && holds(&entry.path(), other))
    })
}

/// Exchanges the folders `staged` and `place`, in one step: there is no moment
/// without a whole runtime at `place`.
fn exchange(staged: &Path, place: &Path) -> Result<(), Failure> {
    use rustix::{
        fs::{CWD, RenameFlags, renameat_with},
        io::Errno,
    };
    #[cfg(test)]
    if tests::no_exchange(place) {
        return Err(Failure::Exchange(place.to_owned()));
    }
    renameat_with(CWD, staged, CWD, place, RenameFlags::EXCHANGE).map_err(|error| match error {
        Errno::NOTSUP | Errno::INVAL | Errno::OPNOTSUPP => Failure::Exchange(place.to_owned()),
        error => std::io::Error::from(error).into(),
    })
}

/// Whether the disk of `root` exchanges two folders, found before a download:
/// one that does not is never written over (`Failure::Exchange`).
fn exchanges(root: &Path) -> Result<(), Failure> {
    let folder = || tempfile::Builder::new().prefix(UNFINISHED).tempdir_in(root);
    let (one, other) = (folder()?, folder()?);
    match exchange(one.path(), other.path()) {
        Err(Failure::Exchange(_)) => Err(Failure::Exchange(root.to_owned())),
        done => done,
    }
}

/// Removes the releases in `root` games no longer run in: all but the newest
/// Obelisk runs, beside the development runtime, the last feed read, the
/// player's choice of beta releases and unfinished installs. `runtimes` calls
/// it once a launch finds no Game session, of any storefront, that may run a
/// game from one.
pub fn remove_replaced(root: &Path) {
    let Some((_, kept)) = newest(root, &Offer::read(root)) else {
        return;
    };
    // One that lost its Wine is to be laid again first: until then no other goes.
    if !has_wine(&kept) {
        return;
    }
    let kept = kept
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let replaced = |name: &str| {
        name != DEVELOPMENT
            && Some(name) != kept.as_deref()
            && name != feed::KEPT
            && name != feed::PRERELEASES
            && !name.starts_with(UNFINISHED)
    };
    let any = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| replaced(&entry.file_name().to_string_lossy()));
    if !any {
        return;
    }
    // Held until they are gone, so that no launch prepares in one; while a
    // launch or a game holds the library they all stay.
    let Ok(Some(library)) = engine_patch::Alone::hold(root) else {
        return;
    };
    if !library.idle() {
        return;
    }
    let mut held = Vec::new();
    remove_entries(root, |name| {
        // A release a game of an earlier build still holds stays.
        if replaced(name) && numbers(name).is_some() {
            match library.release(&root.join(name)) {
                Ok(Some(release)) => held.push(release),
                Ok(None) | Err(_) => return false,
            }
        }
        replaced(name)
    });
}

/// Removes what `root` holds under the names `remove` picks. Whatever cannot be
/// removed now goes with a later install.
pub(crate) fn remove_entries(root: &Path, mut remove: impl FnMut(&str) -> bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !remove(&name.to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let _ = match entry.file_type() {
            Ok(kind) if kind.is_dir() => fs::remove_dir_all(path),
            _ => fs::remove_file(path),
        };
    }
}

#[cfg(test)]
mod tests;
