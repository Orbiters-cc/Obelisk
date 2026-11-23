//! The files a game cannot start without in a runtime: what its engine and the
//! backend it renders with are started from. Looked at as each game starts,
//! which then costs a few dozen looks at names: that a file is there, is one,
//! has its size and can be run. Its content is proven by the archive's own
//! digests when it is laid (`members.rs`), and for our engine files by theirs
//! at every start.
use super::{LINUX, WINE, engine_patch, has_wine, is_release, release_of};
use crate::wine::{Engine, Graphics};
use serde::Deserialize;
use std::{fs, path::{Path, PathBuf}, sync::OnceLock};

/// What every release's Linux engine starts a game from (`launch.sh`), and
/// whether it is run.
const ENGINE: [(&str, bool); 8] = [
    ("engine/fexdarwin-run", true),
    ("engine/wsd", true),
    ("engine/wsdctl", true),
    ("engine/run-with-timeout.sh", true),
    ("wine/bin/wine", true),
    ("wine/bin/wineserver", true),
    ("wine/lib/wine/x86_64-unix/ntdll.so", false),
    ("wine/lib/wine/x86_64-unix/wineobelisk.so", false),
];
const DXMT: [(&str, bool); 3] = [
    ("graphics/bin/virgl_test_server", true),
    ("graphics/libexec/virgl_render_server", true),
    ("graphics/dxmt-native.framework/Versions/A/dxmt-native", false),
];
const D3DMETAL: [(&str, bool); 3] = [
    ("graphics/x86_64/bin/virgl_test_server", true),
    ("graphics/x86_64/libexec/virgl_render_server", true),
    ("graphics/x86_64/lib/libd3dmetal-native.dylib", false),
];

/// Those of the release our engine files are for, with their sizes, as
/// `scripts/engine_required_files.py` reads them from its archive.
const RELEASE: &str = include_str!("../../voile-required.json");

#[derive(Deserialize)]
struct Release {
    voile: String,
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    /// In the release.
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    executable: bool,
    /// Where it leads, for a link.
    #[serde(default)]
    link: Option<PathBuf>,
}

fn built_in() -> &'static Release {
    static VALUE: OnceLock<Release> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(RELEASE).expect("checked-in required files"))
}

impl File {
    /// Whether a game of `engine` that renders with `graphics` needs it.
    fn needed(&self, engine: Engine, graphics: Graphics) -> bool {
        let within = |folder: &str| self.path.starts_with(folder);
        if within("linux/graphics/x86_64/") {
            return engine == Engine::Linux && graphics == Graphics::D3DMetal;
        }
        if within("linux/graphics/") {
            return engine == Engine::Linux && graphics == Graphics::Dxmt;
        }
        if within("linux/") {
            return engine == Engine::Linux;
        }
        match within("lib/external/") {
            true => graphics == Graphics::D3DMetal,
            false => engine == Engine::Rosetta,
        }
    }

    /// Whether it is in `release` as that release ships it, as far as its name
    /// tells.
    fn there(&self, release: &Path) -> bool {
        let path = release.join(&self.path);
        if let Some(link) = &self.link {
            return fs::read_link(&path).is_ok_and(|target| target == *link);
        }
        // Ours in its place has a size of its own, and is proven by its digest.
        let patched = self.path.strip_prefix("linux/").is_some_and(engine_patch::proven);
        fs::symlink_metadata(&path).is_ok_and(|found| {
            found.is_file() && (patched || found.len() == self.size) && (!self.executable || runs(&path))
        })
    }
}

/// Whether this process can run the file at `path`, as its starts will: its
/// mode has an owner, a group and others, and only what is this user's counts.
fn runs(path: &Path) -> bool {
    use rustix::fs::{Access, AtFlags, CWD, accessat};
    accessat(CWD, path, Access::EXEC_OK, AtFlags::EACCESS).is_ok()
}

/// The files a game of `engine` that renders with `graphics` cannot start
/// without that `runtime` lacks, or holds damaged, by their path in it.
pub(crate) fn lacking(runtime: &Path, engine: Engine, graphics: Graphics) -> Vec<String> {
    let mut lacking = Vec::new();
    if engine == Engine::Linux {
        let backend = match graphics {
            Graphics::Dxmt => DXMT,
            Graphics::D3DMetal => D3DMETAL,
        };
        for (path, run) in ENGINE.into_iter().chain(backend) {
            let path_in = runtime.join(path);
            let there = fs::metadata(&path_in)
                .is_ok_and(|found| found.is_file() && found.len() > 0 && (!run || runs(&path_in)));
            if !there {
                lacking.push(path.to_owned());
            }
        }
    }
    let (release, inside) = match engine {
        Engine::Linux => (runtime.parent().unwrap_or(runtime), "linux/"),
        Engine::Rosetta => (runtime, ""),
    };
    // Its own Wine, which every release has, is how one is known to be there.
    if is_release(release) && !has_wine(release) {
        lacking.push(WINE.to_owned());
    }
    let known = built_in();
    if release_of(runtime).as_deref() == Some(known.voile.as_str()) {
        for file in &known.files {
            if file.needed(engine, graphics) && !file.there(release) {
                let path = file.path.strip_prefix(inside).unwrap_or(&file.path);
                if !lacking.iter().any(|known| known == path) {
                    lacking.push(path.to_owned());
                }
            }
        }
    }
    lacking
}

/// Whether `release`, in its folder, holds what games of every engine and
/// backend start from: a release that carries a Linux engine of ours.
pub(super) fn complete(release: &Path) -> bool {
    [Graphics::Dxmt, Graphics::D3DMetal].into_iter().all(|graphics| {
        lacking(&release.join(LINUX), Engine::Linux, graphics).is_empty()
            && lacking(release, Engine::Rosetta, graphics).is_empty()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Lays in the Linux engine `linux` what a game of any release starts
    /// from, each a placeholder, where it is not there yet.
    pub(crate) fn lay(linux: &Path) {
        for (path, run) in ENGINE.into_iter().chain(DXMT).chain(D3DMETAL) {
            let path = linux.join(path);
            if !fs::metadata(&path).is_ok_and(|found| found.len() > 0) {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "placeholder").unwrap();
            }
            let mode = if run { 0o755 } else { 0o644 };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    /// Lays in `release` the files of the release our engine files are for:
    /// each its size, nothing more.
    pub(crate) fn lay_release(release: &Path) {
        let known = built_in();
        fs::create_dir_all(release.join("bin")).unwrap();
        fs::write(release.join("runtime.json"), format!(r#"{{"version": "{}"}}"#, known.voile)).unwrap();
        for file in &known.files {
            let path = release.join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if let Some(link) = &file.link {
                std::os::unix::fs::symlink(link, &path).unwrap();
                continue;
            }
            fs::File::create(&path).unwrap().set_len(file.size).unwrap();
            let mode = if file.executable { 0o755 } else { 0o644 };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    /// The release our engine files are for lists what `launch.sh` starts.
    #[test]
    fn the_required_files_are_of_the_release_our_engine_files_are_for() {
        let known = built_in();
        assert_eq!(known.voile, engine_patch::release());
        for (path, run) in ENGINE.into_iter().chain(DXMT).chain(D3DMETAL) {
            let file = known.files.iter().find(|file| file.path == format!("linux/{path}"));
            let file = file.unwrap_or_else(|| panic!("{path} is not listed"));
            assert!(file.link.is_some() || (file.size > 0 && file.executable >= run), "{path}");
        }
        for path in ["bin/wine", "bin/wineserver", "lib/external/D3DMetal.framework/Versions/A/D3DMetal", "linux/windows/d3d11.dll"] {
            assert!(known.files.iter().any(|file| file.path == path), "{path}");
        }
    }

    /// Seventh verification, blocker 3 and its first major (the verifier's
    /// broker fixture): whatever a start needs that is gone, cut short, no
    /// file, or not to be run, is named, for the backend that needs it only.
    #[test]
    fn a_runtime_names_what_a_game_cannot_start_without() {
        let directory = tempfile::tempdir().unwrap();
        let release = directory.path().join(engine_patch::release());
        lay_release(&release);
        let linux = release.join(LINUX);
        assert!(complete(&release));
        let broker = "graphics/bin/virgl_test_server";
        for spoil in [
            |path: &Path| fs::remove_file(path),
            |path: &Path| fs::File::options().write(true).open(path)?.set_len(7),
            |path: &Path| fs::set_permissions(path, fs::Permissions::from_mode(0o644)),
            // (eighth verification: run by anyone but its owner)
            |path: &Path| fs::set_permissions(path, fs::Permissions::from_mode(0o645)),
            |path: &Path| {
                fs::remove_file(path)?;
                fs::create_dir(path)
            },
        ] {
            let path = linux.join(broker);
            spoil(&path).unwrap();
            assert_eq!(lacking(&linux, Engine::Linux, Graphics::Dxmt), [broker]);
            assert!(lacking(&linux, Engine::Linux, Graphics::D3DMetal).is_empty());
            assert!(lacking(&release, Engine::Rosetta, Graphics::Dxmt).is_empty());
            assert!(!complete(&release));
            let _ = fs::remove_dir_all(&release);
            lay_release(&release);
        }
        for path in ["wine/bin/wine", "wine/lib/wine/x86_64-unix/wine", "engine/wsdctl", "windows/dxgi.dll"] {
            fs::remove_file(linux.join(path)).unwrap();
            let named = lacking(&linux, Engine::Linux, Graphics::Dxmt);
            assert!(named.iter().any(|named| named == path), "{path}: {named:?}");
        }
        fs::remove_file(release.join("bin/wineserver")).unwrap();
        assert_eq!(lacking(&release, Engine::Rosetta, Graphics::Dxmt), ["bin/wineserver"]);
        // A release this program knows no sizes of: what every one starts from.
        let other = directory.path().join("v9.0.0-1/linux");
        lay(&other);
        assert!(lacking(&other, Engine::Linux, Graphics::Dxmt).is_empty());
        fs::write(other.join(broker), "").unwrap();
        fs::remove_file(other.join("wine/bin/wineserver")).unwrap();
        assert_eq!(lacking(&other, Engine::Linux, Graphics::Dxmt), ["wine/bin/wineserver", broker]);
    }
}
