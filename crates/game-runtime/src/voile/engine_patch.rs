//! The Orbiters fork's engine files for the keyboard layout. Voile's Linux
//! engine takes every Mac keyboard for a US one: its window server names a
//! key by its place, and its Wine driver has no layout. Two files of ours,
//! built from Voile's own sources plus the layout (`scripts/
//! build-engine-patch.sh`, `engine-patch/manifest.json`), replace the
//! release's own before a game starts on the engine: the window server then
//! tells the layout selected on the Mac, and the driver gives the game its
//! virtual keys and characters.
//!
//! They are built for one release and go over that release only, and only
//! over the files it shipped, told by their SHA-256: any other release, a
//! development runtime, or a file that is neither Voile's nor ours is left
//! as it is, and its games run with a US keyboard as before. The window
//! server and the driver each run with the other's original: a patch
//! interrupted between its two files leaves an engine that starts, and the
//! next launch completes it.
use serde::Deserialize;
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, sync::OnceLock};

const MANIFEST: &str = include_str!("../../engine-patch/manifest.json");
/// The manifest's files, in its order.
const FILES: [&[u8]; 2] = [
    include_bytes!("../../engine-patch/wsd"),
    include_bytes!("../../engine-patch/wineobelisk.so"),
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
    /// The SHA-256 of the file the release ships.
    original: String,
    /// The SHA-256 of ours.
    patched: String,
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
                .zip(FILES)
                .map(|(entry, bytes)| File {
                    path: entry.path,
                    original: entry.original,
                    patched: entry.patched,
                    bytes,
                })
                .collect(),
        }
    })
}

/// Which keyboard the games of a Linux engine runtime get.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// Our files are in place: the layout selected on the Mac.
    Mac,
    /// The release is the one our files are for and still has some of its
    /// own: the next launch on the engine puts ours in place.
    Pending,
    /// Another release, or files that are neither the release's nor ours: a
    /// US keyboard, whatever the Mac's layout.
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
#[derive(PartialEq, Eq)]
enum Found {
    Original,
    Patched,
    Other,
}

impl Patch {
    fn found(&self, linux: &Path) -> Vec<Found> {
        self.files
            .iter()
            .map(|file| match crate::sha(&linux.join(&file.path)) {
                Ok(sha) if sha == file.patched => Found::Patched,
                Ok(sha) if sha == file.original => Found::Original,
                _ => Found::Other,
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

    /// Puts our files over the release's own in `linux`, when every one of
    /// them is the release's or already ours. Each goes in whole or not at
    /// all: written beside the file it replaces, then renamed over it, which
    /// leaves a running game the file it started with.
    fn apply(&self, linux: &Path) -> Keyboard {
        if self.keyboard(linux) != Keyboard::Pending {
            return self.keyboard(linux);
        }
        for (file, found) in self.files.iter().zip(self.found(linux)) {
            if found == Found::Original && replace(&linux.join(&file.path), file.bytes).is_err() {
                break;
            }
        }
        self.keyboard(linux)
    }
}

fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = path.parent().ok_or(std::io::ErrorKind::NotFound)?;
    let mode = fs::metadata(path)?.permissions().mode();
    let mut file = tempfile::Builder::new()
        .prefix(".obelisk-")
        .tempfile_in(folder)?;
    file.write_all(bytes)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|failed| failed.error)?;
    Ok(())
}

/// Which keyboard the games of the Linux engine runtime `linux` get.
pub fn keyboard(linux: &Path) -> Keyboard {
    built_in().keyboard(linux)
}

/// Puts our engine files in place in `linux` when they are for its release,
/// before a game starts there. Never fails a launch: a runtime it leaves
/// alone, or could not write to, runs as Voile shipped it.
pub(crate) fn apply(linux: &Path) -> Keyboard {
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

    /// A patch whose originals are `voile's wsd` and `voile's driver`.
    fn patch() -> Patch {
        let file = |path: &str, original: &[u8], bytes: &'static [u8]| File {
            path: path.to_owned(),
            original: sha(original),
            patched: sha(bytes),
            bytes,
        };
        Patch {
            release: RELEASE.to_owned(),
            files: vec![
                file(WSD, b"voile's wsd", b"our wsd"),
                file(DRIVER, b"voile's driver", b"our driver"),
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
        fs::write(
            release.join("runtime.json"),
            format!(r#"{{"version":"{version}","contract":1}}"#),
        )
        .unwrap();
        fs::write(linux.join(WSD), b"voile's wsd").unwrap();
        fs::set_permissions(linux.join(WSD), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(linux.join(DRIVER), b"voile's driver").unwrap();
        (root, linux)
    }

    #[test]
    fn the_files_built_in_are_the_ones_the_manifest_names() {
        let patch = built_in();
        assert!(crate::voile::numbers(&patch.release).is_some());
        assert_eq!(patch.files.len(), FILES.len());
        assert_eq!(patch.files[0].path, WSD);
        assert_eq!(patch.files[1].path, DRIVER);
        for file in &patch.files {
            assert_eq!(sha(file.bytes), file.patched, "{}", file.path);
            assert_ne!(file.original, file.patched, "{}", file.path);
        }
    }

    #[test]
    fn our_files_replace_the_release_s_own_and_keep_their_mode() {
        let (_root, linux) = runtime(RELEASE);
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"our wsd");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
        let mode = fs::metadata(linux.join(WSD)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        // Nothing is left beside them.
        let left: Vec<_> = fs::read_dir(linux.join("engine"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(left.len(), 1);
        // And a second launch finds them in place.
        assert_eq!(patch().apply(&linux), Keyboard::Mac);
        assert_eq!(patch().keyboard(&linux), Keyboard::Mac);
    }

    #[test]
    fn another_release_is_left_as_it_is() {
        let (_root, linux) = runtime("v1.0.0-2");
        assert_eq!(patch().apply(&linux), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
    }

    #[test]
    fn a_file_that_is_neither_voile_s_nor_ours_leaves_every_file_alone() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(DRIVER), b"someone else's driver").unwrap();
        assert_eq!(patch().apply(&linux), Keyboard::Us);
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
        assert_eq!(patch().apply(&linux), Keyboard::Us);
        assert!(!linux.join(WSD).exists());
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"voile's driver");
    }

    #[test]
    fn a_patch_interrupted_between_its_files_is_completed() {
        let (_root, linux) = runtime(RELEASE);
        fs::write(linux.join(WSD), b"our wsd").unwrap();
        assert_eq!(patch().keyboard(&linux), Keyboard::Pending);
        assert_eq!(patch().apply(&linux), Keyboard::Mac);
        assert_eq!(fs::read(linux.join(DRIVER)).unwrap(), b"our driver");
    }

    #[test]
    fn a_runtime_that_cannot_be_written_stays_pending() {
        let (_root, linux) = runtime(RELEASE);
        let engine = linux.join("engine");
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(patch().apply(&linux), Keyboard::Pending);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_runtime_without_a_manifest_is_left_as_it_is() {
        let (root, linux) = runtime(RELEASE);
        fs::remove_file(root.path().join(RELEASE).join("runtime.json")).unwrap();
        assert_eq!(patch().apply(&linux), Keyboard::Us);
        assert_eq!(fs::read(linux.join(WSD)).unwrap(), b"voile's wsd");
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
        assert_eq!(apply(linux), Keyboard::Mac);
        assert_eq!(keyboard(linux), Keyboard::Mac);
    }
}
