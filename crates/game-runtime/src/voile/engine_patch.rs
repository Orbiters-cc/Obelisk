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
use serde::Deserialize;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Original,
    /// Ours, from an earlier obelisk-cli: replaced as the release's own is.
    Previous,
    Patched,
    Other,
}

/// The engine files could not be put back as they were after a switch failed: the runtime holds some files
/// of one set and some of another, which no game may start on.
#[derive(Debug, PartialEq, Eq)]
pub struct Incoherent {
    /// The paths, in the runtime, that are not what they were.
    pub paths: Vec<String>,
}

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

impl Patch {
    fn found(&self, linux: &Path) -> Vec<Found> {
        self.files
            .iter()
            .map(|file| match crate::sha(&linux.join(&file.path)) {
                Ok(sha) => file.found_sha(&sha),
                Err(_) => Found::Other,
            })
            .collect()
    }

    fn keyboard(&self, linux: &Path) -> Keyboard {
        if crate::voile::release_of(linux).as_deref() != Some(self.release.as_str()) {
            return Keyboard::Us;
        }
        let found = self.found(linux);
        if found.contains(&Found::Other) {
            Keyboard::Us
        } else if found.iter().all(|found| *found == Found::Patched) {
            Keyboard::Mac
        } else {
            Keyboard::Pending
        }
    }

    /// What each file of the runtime is, as `status` tells it: `voile`,
    /// `ours` or `other`, by its path in the runtime.
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
                };
                (file.path.clone(), state)
            })
            .collect()
    }

    /// Puts our files over the release's own (or an earlier obelisk-cli's) in `linux`, all of them or none,
    /// when every one of them is the release's or ours. `Err` when a switch failed and could not be undone:
    /// the runtime then holds a mix no game may start on.
    fn apply(&self, linux: &Path) -> Result<Keyboard, Incoherent> {
        self.apply_failing_at(linux, None, false)
    }

    /// [`Patch::apply`], with the switch of file `fail_at` failing, and when `undo_fails` the putting back too
    /// (a test's).
    fn apply_failing_at(&self, linux: &Path, fail_at: Option<usize>, undo_fails: bool) -> Result<Keyboard, Incoherent> {
        let Ok(_lock) = lock(linux) else { return Ok(self.keyboard(linux)) };
        // A switch a crash cut short is undone first: the originals kept aside go back.
        restore(self, linux)?;
        // One look, under the lock: what it sees is what is changed.
        if self.keyboard(linux) != Keyboard::Pending {
            return Ok(self.keyboard(linux));
        }
        let before: Vec<Option<String>> = self.files.iter().map(|file| crate::sha(&linux.join(&file.path)).ok()).collect();
        let found = self.found(linux);
        let mut staged = Vec::new();
        for (file, found) in self.files.iter().zip(found) {
            if !matches!(found, Found::Original | Found::Previous) {
                continue;
            }
            match stage(&linux.join(&file.path), file.bytes, &file.patched) {
                Ok(temp) => staged.push((file, temp)),
                // Nothing switched yet: the staged files go with their temporaries.
                Err(_) => return Ok(self.keyboard(linux)),
            }
        }
        // Each file replaced kept aside, then all switched.
        let mut kept = Vec::new();
        for (file, _) in &staged {
            let path = linux.join(&file.path);
            let aside = aside_of(&path);
            let _ = fs::remove_file(&aside);
            if fs::hard_link(&path, &aside).is_err() {
                undo(&kept, false);
                return self.coherent_after_undo(linux, &before);
            }
            kept.push((path, aside));
        }
        for (index, (file, temp)) in staged.into_iter().enumerate() {
            let path = linux.join(&file.path);
            let done = if fail_at == Some(index) {
                Err(std::io::Error::other("a test's failure"))
            } else {
                temp.persist(&path).map(|_| ()).map_err(|failed| failed.error)
            };
            if done.is_err() {
                undo(&kept, undo_fails);
                return self.coherent_after_undo(linux, &before);
            }
        }
        for (_, aside) in &kept {
            let _ = fs::remove_file(aside);
        }
        Ok(self.keyboard(linux))
    }

    /// After a failed switch was undone: every file must be what it was before, or the set is incoherent.
    fn coherent_after_undo(&self, linux: &Path, before: &[Option<String>]) -> Result<Keyboard, Incoherent> {
        let paths: Vec<String> = self
            .files
            .iter()
            .zip(before)
            .filter(|(file, before)| crate::sha(&linux.join(&file.path)).ok() != **before)
            .map(|(file, _)| file.path.clone())
            .collect();
        if paths.is_empty() { Ok(self.keyboard(linux)) } else { Err(Incoherent { paths }) }
    }
}

/// Where the original of `path` is kept during a switch.
fn aside_of(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".obelisk-original-{name}"))
}

/// Puts back every original kept aside in `kept` (`fail`: a test's failure to do so).
fn undo(kept: &[(std::path::PathBuf, std::path::PathBuf)], fail: bool) {
    for (path, aside) in kept {
        if !fail {
            let _ = put_back(path, aside);
        }
    }
}

/// The original kept at `aside` goes back to `path`; when `path` is that same file still (it was never switched),
/// the second name goes (a rename between two names of one file does nothing).
fn put_back(path: &Path, aside: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let same = matches!((fs::metadata(path), fs::metadata(aside)), (Ok(a), Ok(b)) if (a.dev(), a.ino()) == (b.dev(), b.ino()));
    if same { fs::remove_file(aside) } else { fs::rename(aside, path) }
}

/// Undoes a switch a crash cut short: every file still kept aside goes back, when it is one the release or an
/// earlier obelisk-cli shipped there (a stale or foreign file under that name is removed, never installed);
/// temporaries a crash left beside the files go. `Err` when a known file could not be put back.
fn restore(patch: &Patch, linux: &Path) -> Result<(), Incoherent> {
    let mut failed = Vec::new();
    for file in &patch.files {
        let path = linux.join(&file.path);
        let aside = aside_of(&path);
        if aside.is_file() {
            let known = crate::sha(&aside).map(|sha| matches!(file.found_sha(&sha), Found::Original | Found::Previous));
            let done = if known.unwrap_or(false) { put_back(&path, &aside) } else { fs::remove_file(&aside) };
            if done.is_err() {
                failed.push(file.path.clone());
            }
        }
        if let Some(folder) = path.parent() {
            for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(".obelisk-") && !name.starts_with(".obelisk-original-") && name != ".obelisk-engine-patch.lock" {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    if failed.is_empty() { Ok(()) } else { Err(Incoherent { paths: failed }) }
}

/// The runtime's lock for its engine files, held while the file lives.
fn lock(linux: &Path) -> std::io::Result<fs::File> {
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(linux.join(".obelisk-engine-patch.lock"))?;
    file.lock()?;
    Ok(file)
}

/// Our file written beside `path` with its mode, checked against `sha` before anything is switched.
fn stage(path: &Path, bytes: &[u8], sha: &str) -> std::io::Result<tempfile::NamedTempFile> {
    let folder = path.parent().ok_or(std::io::ErrorKind::NotFound)?;
    let mode = fs::metadata(path)?.permissions().mode();
    let mut file = tempfile::Builder::new().prefix(".obelisk-").tempfile_in(folder)?;
    file.write_all(bytes)?;
    file.as_file().set_permissions(fs::Permissions::from_mode(mode))?;
    file.as_file().sync_all()?;
    if crate::sha(file.path()).ok().as_deref() != Some(sha) {
        return Err(std::io::Error::other("the staged file is not ours"));
    }
    Ok(file)
}

/// Which keyboard the games of the Linux engine runtime `linux` get.
pub fn keyboard(linux: &Path) -> Keyboard {
    built_in().keyboard(linux)
}

/// What each engine file of the runtime `linux` is (`voile`, `ours`, `other`), by its path in the runtime.
pub fn components(linux: &Path) -> Vec<(String, &'static str)> {
    built_in().components(linux)
}

/// Puts our engine files in place in `linux` when they are for its release,
/// before a game starts there. A runtime it leaves alone, or could not write
/// to, runs as it is (Voile's files, or a whole earlier set of ours); `Err`
/// only when a failed switch left a mix of two sets, which no game may start on.
pub(crate) fn apply(linux: &Path) -> Result<Keyboard, Incoherent> {
    built_in().apply(linux)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
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
        assert_eq!(patch().apply_failing_at(&linux, Some(1), false).unwrap(), Keyboard::Pending);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd", "the first one switched back");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
        assert_eq!(fs::read(linux.join(SERVER)).unwrap(), b"voile's server");
        assert!(patch().components(&linux).iter().all(|(_, state)| *state == "voile"));
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
        assert_eq!(apply(linux).unwrap(), Keyboard::Mac);
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
        let result = patch().apply_failing_at(&linux, Some(1), true);
        assert_eq!(result, Err(Incoherent { paths: vec![WSD.to_owned()] }));
        // The next launch puts the original back from where it was kept, then patches all.
        assert_eq!(patch().apply(&linux).unwrap(), Keyboard::Mac);
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
}
