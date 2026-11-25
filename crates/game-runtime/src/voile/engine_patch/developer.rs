//! The timeline generation: the engine files with instrumented host programs in place of four of them, for a developer's
//! measurements (`docs/CLI.md`, "The timeline generation"). Only a build with the feature `developer-timeline` knows it, and
//! only a process asked for it ([`select`]) puts it in place or starts a game on it; the same build not asked for it counts
//! its files as an earlier set of ours, which the released one replaces before any game starts. Both ways are the one
//! transaction of every engine file, under the same locks, with the same proof.
//!
//! Its files are not in the program: they come from the folder `scripts/build-timeline-generation.sh` wrote, each taken only
//! when it has the digest built in here (`engine-patch/timeline.json`).
use super::{Entry, FILES, File, Incoherent, Manifest, Patch, Published, read_sha};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const MANIFEST: &str = include_str!("../../../engine-patch/timeline.json");

/// Which engine files this process puts in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    Released,
    Timeline,
}

impl Generation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::Timeline => "timeline",
        }
    }
}

#[derive(Deserialize)]
struct Timeline {
    voile: String,
    files: Vec<Instrumented>,
}

#[derive(Deserialize)]
struct Instrumented {
    path: String,
    /// Its name in the folder.
    source: String,
    /// Voile's own file.
    original: String,
    /// The released set's: Voile's own, or ours.
    released: String,
    timeline: String,
}

struct Selected {
    generation: Generation,
    folder: Option<PathBuf>,
}

static SELECTED: OnceLock<Selected> = OnceLock::new();

/// What this process is asked for, once, before anything looks at an engine file; `folder` holds the generation's files, and
/// the released ones only it changes (`original/`). `Err` when it is not that generation's folder.
pub fn select(generation: Generation, folder: Option<&Path>) -> Result<(), String> {
    let manifest = manifest();
    if generation == Generation::Timeline {
        let folder = folder.ok_or("the timeline generation needs its folder")?;
        for file in &manifest.files {
            let path = folder.join(&file.source);
            if read_sha(&path) != Ok(Some(file.timeline.clone())) {
                return Err(format!("{} is not this build's timeline generation's", path.display()));
            }
        }
    }
    let selected = Selected { generation, folder: folder.map(Path::to_owned) };
    SELECTED.set(selected).map_err(|_| "the generation was picked before".to_owned())
}

/// What this process was asked for.
pub fn selected() -> Generation {
    SELECTED.get().map_or(Generation::Released, |selected| selected.generation)
}

fn manifest() -> Timeline {
    serde_json::from_str(MANIFEST).expect("checked-in timeline generation")
}

/// The files and sets of the built-in engine patch, as this process was asked for.
pub(super) fn patch(released: Manifest) -> Patch {
    let selected = SELECTED.get();
    let folder = selected.and_then(|selected| selected.folder.as_deref());
    patch_of(released, manifest(), selected.map_or(Generation::Released, |s| s.generation), folder)
}

fn patch_of(released: Manifest, timeline: Timeline, generation: Generation, folder: Option<&Path>) -> Patch {
    assert_eq!(released.voile, timeline.voile, "the timeline generation is of another release");
    let mut entries: Vec<Entry> = released.files;
    // The files it alone changes are Voile's own in every released set.
    for file in timeline.files.iter().filter(|file| entries.iter().all(|entry| entry.path != file.path)).collect::<Vec<_>>() {
        assert_eq!(file.original, file.released, "{}", file.path);
        entries.push(Entry {
            path: file.path.clone(),
            source: String::new(),
            original: file.original.clone(),
            patched: file.original.clone(),
            previous: Vec::new(),
        });
    }
    let instrumented = |path: &str| timeline.files.iter().find(|file| file.path == path);
    // Every set an update may go from or to: the released ones, and the other of the two.
    let of = |published: &Published| entries.iter().map(|entry| published.files.get(&entry.path).unwrap_or(&entry.original).clone()).collect::<Vec<_>>();
    let mut published: Vec<Vec<String>> = released.generations.iter().map(of).collect();
    let ours: Vec<String> = entries.iter().map(|entry| entry.patched.clone()).collect();
    let theirs: Vec<String> = entries.iter().map(|entry| instrumented(&entry.path).map_or(&entry.patched, |file| &file.timeline).clone()).collect();
    published.push(match generation {
        Generation::Released => theirs,
        Generation::Timeline => ours,
    });
    let files = entries
        .into_iter()
        .map(|entry| {
            let file = instrumented(&entry.path);
            if let Some(file) = file {
                assert!(file.released == entry.patched && file.original == entry.original, "{}: not the released set's file", entry.path);
            }
            let mut previous = entry.previous;
            let built_in = FILES.iter().find(|(source, _)| *source == entry.source).map(|(_, bytes)| *bytes);
            let (patched, bytes) = match (generation, file) {
                (Generation::Timeline, Some(file)) => {
                    previous.push(entry.patched.clone());
                    (file.timeline.clone(), read(folder, &file.source, &file.timeline))
                }
                (Generation::Released, Some(file)) => {
                    previous.push(file.timeline.clone());
                    let original = || read(folder, &format!("original/{}", file.source), &file.original);
                    (entry.patched.clone(), built_in.unwrap_or_else(original))
                }
                (_, None) => (entry.patched.clone(), built_in.expect("every file the manifest names is built in")),
            };
            File { path: entry.path, original: entry.original, patched, previous, bytes }
        })
        .collect();
    Patch { release: released.voile, files, published, idle: super::super::live::idle }
}

/// A file of the folder, for as long as this process lives; nothing when it is not the one named: what is put in place is
/// checked by its digest, and nothing never has it.
fn read(folder: Option<&Path>, name: &str, sha: &str) -> &'static [u8] {
    let path = folder.map(|folder| folder.join(name));
    match path {
        Some(path) if read_sha(&path) == Ok(Some(sha.to_owned())) => Box::leak(fs::read(path).unwrap_or_default().into_boxed_slice()),
        _ => &[],
    }
}

/// A game starts on the very set this process was asked for: the timeline's whole, or a set with nothing of it (the patch's own
/// proof says which are whole).
pub(super) fn proven(patch: &Patch, linux: &Path) -> Result<(), Incoherent> {
    proven_as(patch, linux, &manifest(), selected())
}

fn proven_as(patch: &Patch, linux: &Path, timeline: &Timeline, generation: Generation) -> Result<(), Incoherent> {
    if crate::voile::release_of(linux).as_deref() != Some(patch.release.as_str()) {
        return match generation {
            Generation::Released => Ok(()),
            Generation::Timeline => Err(patch.refuse(Vec::new(), "the timeline generation is for another release", super::Action::QuitGame)),
        };
    }
    let wrong: Vec<String> = patch
        .files
        .iter()
        .filter(|file| {
            let found = read_sha(&linux.join(&file.path)).ok().flatten().unwrap_or_default();
            match generation {
                Generation::Timeline => found != file.patched,
                Generation::Released => timeline.files.iter().any(|t| t.path == file.path && t.timeline == found),
            }
        })
        .map(|file| file.path.clone())
        .collect();
    match wrong.is_empty() {
        true => Ok(()),
        false => Err(patch.refuse(wrong, "the engine files are not the generation asked for", super::Action::QuitGame)),
    }
}

/// The digests of every file the timeline generation is proven by, as they are in `linux`: for whoever asks what is there.
pub fn digests(linux: &Path) -> Vec<(String, Option<String>)> {
    let patch = super::built_in();
    patch.files.iter().map(|file| (file.path.clone(), read_sha(&linux.join(&file.path)).ok().flatten())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voile::engine_patch::{Crashed, Faults, Found, Keyboard, Phase};
    use sha2::{Digest, Sha256};

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    const RELEASE: &str = "v1.0.0-1";
    /// path, Voile's, the released set's, the timeline's
    const SET: [(&str, &str, &str, &str); 4] = [
        ("engine/wsd", "voile wsd", "our wsd", "timeline wsd"),
        ("engine/fexdarwin-run", "voile engine", "our engine", "our engine"),
        ("graphics/lib/lib.dylib", "voile library", "voile library", "timeline library"),
        ("graphics/dxmt", "voile dxmt", "voile dxmt", "timeline dxmt"),
    ];

    fn released() -> Manifest {
        let files = SET[..2].iter().map(|(path, voile, ours, _)| {
            serde_json::json!({"path": path, "source": "wsd", "original": sha(voile.as_bytes()), "patched": sha(ours.as_bytes())})
        });
        serde_json::from_value(serde_json::json!({"voile": RELEASE, "files": files.collect::<Vec<_>>()})).unwrap()
    }

    fn timeline() -> Timeline {
        let files = SET.iter().filter(|(_, _, ours, timeline)| ours != timeline).map(|(path, voile, ours, timeline)| {
            let source = Path::new(path).file_name().unwrap().to_string_lossy();
            serde_json::json!({"path": path, "source": source, "original": sha(voile.as_bytes()), "released": sha(ours.as_bytes()), "timeline": sha(timeline.as_bytes())})
        });
        serde_json::from_value(serde_json::json!({"voile": RELEASE, "files": files.collect::<Vec<_>>()})).unwrap()
    }

    /// A patch as a process asked for `generation` has it, its built-in files these tests'.
    fn patch(generation: Generation, folder: Option<&Path>) -> Patch {
        let mut patch = patch_of(released(), timeline(), generation, folder);
        patch.idle = |_| Ok(());
        for file in &mut patch.files {
            let (_, _, ours, _) = SET.iter().find(|(path, ..)| *path == file.path).unwrap();
            if file.patched == sha(ours.as_bytes()) && (file.path.starts_with("engine/")) {
                file.bytes = ours.as_bytes();
            }
        }
        patch
    }

    struct Place {
        _root: tempfile::TempDir,
        linux: PathBuf,
        folder: PathBuf,
    }

    fn place() -> Place {
        let root = tempfile::tempdir().unwrap();
        let linux = root.path().join("Voile").join(RELEASE).join("linux");
        let folder = root.path().join("generation");
        for (path, voile, _, timeline) in SET {
            let there = linux.join(path);
            fs::create_dir_all(there.parent().unwrap()).unwrap();
            fs::write(there, voile).unwrap();
            let name = Path::new(path).file_name().unwrap();
            fs::create_dir_all(folder.join("original")).unwrap();
            fs::write(folder.join(name), timeline).unwrap();
            fs::write(folder.join("original").join(name), voile).unwrap();
        }
        fs::write(linux.parent().unwrap().join("runtime.json"), format!(r#"{{"version": "{RELEASE}"}}"#)).unwrap();
        Place { _root: root, linux, folder }
    }

    impl Place {
        fn holds(&self) -> &'static str {
            let is = |pick: fn(&(&str, &'static str, &'static str, &'static str)) -> &'static str| {
                SET.iter().all(|file| fs::read_to_string(self.linux.join(file.0)).unwrap_or_default() == pick(file))
            };
            match (is(|f| f.1), is(|f| f.2), is(|f| f.3)) {
                (true, ..) => "voile",
                (_, true, _) => "released",
                (.., true) => "timeline",
                _ => "mixture",
            }
        }
        /// As a launch does it: the files put in place, then held to be the generation asked for.
        fn launch(&self, generation: Generation) -> Result<Keyboard, Incoherent> {
            let patch = patch(generation, Some(&self.folder));
            let (keyboard, pin) = super::super::tests::prepared_for_tests(&patch, &self.linux)?;
            proven_as(&patch, &self.linux, &timeline(), generation)?;
            drop(pin);
            Ok(keyboard)
        }
        fn clean(&self) {
            for folder in ["engine", "graphics", "graphics/lib", ""] {
                for entry in fs::read_dir(self.linux.join(folder)).unwrap().flatten() {
                    assert!(!entry.file_name().to_string_lossy().starts_with(".obelisk-txn"), "{:?}", entry.path());
                }
            }
            assert!(!self.linux.join(super::super::JOURNAL).exists());
        }
    }

    #[test]
    fn the_timeline_goes_in_whole_and_out_whole() {
        let place = place();
        place.launch(Generation::Released).unwrap();
        assert_eq!(place.holds(), "released");
        assert_eq!(place.launch(Generation::Timeline).unwrap(), Keyboard::Mac);
        assert_eq!(place.holds(), "timeline");
        place.clean();
        // Again: nothing to change.
        place.launch(Generation::Timeline).unwrap();
        place.launch(Generation::Released).unwrap();
        assert_eq!(place.holds(), "released");
        place.clean();
        // From Voile's own files too.
        for (path, voile, ..) in SET {
            fs::write(place.linux.join(path), voile).unwrap();
        }
        place.launch(Generation::Timeline).unwrap();
        assert_eq!(place.holds(), "timeline");
    }

    #[test]
    fn a_process_not_asked_for_it_starts_nothing_on_it() {
        let place = place();
        place.launch(Generation::Timeline).unwrap();
        // While something holds the files, a released launch cannot put its own back: it does not start on these.
        let mut held = patch(Generation::Released, Some(&place.folder));
        held.idle = |_| Err("a game runs".into());
        let prepared = super::super::tests::prepared_for_tests(&held, &place.linux);
        let refused = prepared.and_then(|_| proven_as(&held, &place.linux, &timeline(), Generation::Released)).unwrap_err();
        assert_eq!(refused.paths.len(), 3, "{refused}");
        assert_eq!(place.holds(), "timeline");
        // Nor without the files only the folder has.
        let without = patch(Generation::Released, None);
        let prepared = super::super::tests::prepared_for_tests(&without, &place.linux);
        assert!(prepared.and_then(|_| proven_as(&without, &place.linux, &timeline(), Generation::Released)).is_err());
        assert_eq!(place.holds(), "timeline");
        place.launch(Generation::Released).unwrap();
        assert_eq!(place.holds(), "released");
    }

    #[test]
    fn a_build_that_does_not_know_it_takes_its_files_for_foreign() {
        let place = place();
        place.launch(Generation::Timeline).unwrap();
        let wsd = released().files.remove(0);
        let file = File { path: wsd.path, original: wsd.original, patched: wsd.patched, previous: wsd.previous, bytes: b"" };
        assert_eq!(file.found_sha(&sha(b"timeline wsd")), Found::Other);
    }

    #[test]
    fn a_crash_on_the_way_in_or_out_is_ended_whole_by_either() {
        for (from, to) in [(Generation::Released, Generation::Timeline), (Generation::Timeline, Generation::Released)] {
            for after in [Generation::Released, Generation::Timeline] {
                for (phase, switched) in [(Phase::Staging, None), (Phase::Switching, None), (Phase::Switching, Some(1)), (Phase::Switching, Some(2)), (Phase::Committed, None)] {
                    let place = place();
                    place.launch(from).unwrap();
                    let cut = patch(to, Some(&place.folder));
                    let faults = Faults { crash_after: Some(phase), crash_switched: switched, ..Faults::default() };
                    let crashed = super::super::tests::prepared_with_for_tests(&cut, &place.linux, faults);
                    assert!(matches!(crashed, Err(Crashed)), "{from:?} {to:?} {phase:?} {switched:?}");
                    place.launch(after).unwrap();
                    assert_eq!(place.holds(), after.name(), "{from:?} {to:?} {phase:?} {switched:?}");
                    place.clean();
                }
            }
        }
    }

    #[test]
    fn a_file_that_is_not_the_folder_s_is_never_put_in_place() {
        let place = place();
        place.launch(Generation::Released).unwrap();
        fs::write(place.folder.join("dxmt"), "something else").unwrap();
        assert!(place.launch(Generation::Timeline).is_err());
        assert_eq!(place.holds(), "released");
        assert!(select(Generation::Timeline, Some(&place.folder)).is_err());
    }

    #[test]
    fn a_foreign_file_is_replaced_by_neither() {
        let place = place();
        place.launch(Generation::Released).unwrap();
        fs::write(place.linux.join("graphics/dxmt"), "foreign").unwrap();
        assert!(place.launch(Generation::Timeline).is_err());
        assert!(place.launch(Generation::Released).is_err());
        assert_eq!(fs::read_to_string(place.linux.join("graphics/dxmt")).unwrap(), "foreign");
    }

    /// The generation checked in is this build's: of its release, over its released set, with nothing of the emulated game's.
    #[test]
    fn the_generation_checked_in_is_of_this_build_s_release() {
        let released: Manifest = serde_json::from_str(super::super::MANIFEST).unwrap();
        let timeline = manifest();
        assert_eq!(timeline.voile, released.voile);
        // (programs on the Mac's side alone: nothing the game runs, loads or maps)
        let host = ["engine/wsd", "graphics/libexec/virgl_render_server", "graphics/x86_64/libexec/virgl_render_server"];
        assert_eq!(timeline.files.iter().map(|f| f.path.as_str()).collect::<std::collections::BTreeSet<_>>(), host.into_iter().collect());
        for file in &timeline.files {
            assert_ne!(file.timeline, file.released);
            match released.files.iter().find(|entry| entry.path == file.path) {
                Some(entry) => assert_eq!((&entry.patched, &entry.original), (&file.released, &file.original)),
                None => assert_eq!(file.released, file.original),
            }
        }
        let patch = patch_of(released, timeline, Generation::Released, None);
        assert_eq!(patch.files.len(), 5);
    }
}
