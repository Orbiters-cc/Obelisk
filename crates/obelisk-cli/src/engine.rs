//! `engine repair`: the Voile release Windows games run in, laid again over
//! the one in place, whatever is missing, foreign or cut short in it. Nothing
//! in place goes before the whole release is there and checked, and nothing
//! while a game runs in it: the runtime's own lock says so (docs/CLI.md).
use crate::{
    events::{EngineFile, Event, Sink, Throttle, fraction},
    failure::{Failure, LAUNCH_FAILED, VOILE_FAILED},
    places::Places,
};
use game_runtime::voile::{self, Install, engine_patch};
use std::{path::Path, time::Instant};

/// What each engine file of this build is in the Linux engine of the Voile
/// in `root`, as events tell it.
pub fn files(root: &Path) -> Option<Vec<EngineFile>> {
    voile::engine_files(root).map(|files| {
        files
            .into_iter()
            .map(|(path, state)| EngineFile { path, state })
            .collect()
    })
}

/// `archive` stands in for the feed and the download, in tests.
pub async fn repair(
    root: &Path,
    places: &Places,
    archive: Option<&Path>,
    out: &dyn Sink,
) -> Result<(), Failure> {
    places.open_library()?;
    std::fs::create_dir_all(root)
        .map_err(|error| Failure::new(VOILE_FAILED, format!("The runtimes cannot be read: {error}.")))?;
    if voile::is_development(root) {
        return Err(Failure::new(
            VOILE_FAILED,
            "A development runtime is in place: no release is installed over it.",
        ));
    }
    let install = Install::new();
    let throttle = Throttle::default();
    let mut run = Box::pin(install.repair(root, archive));
    let done = loop {
        let (bytes_done, bytes_total) = install.progress();
        if throttle.passes("voile", Instant::now()) {
            out.emit(Event::EngineRepairing {
                bytes_done,
                bytes_total,
                fraction: fraction(bytes_done, bytes_total),
            });
        }
        tokio::select! {
            done = &mut run => break done,
            () = tokio::time::sleep(crate::events::PACE) => {}
        }
    };
    done.map_err(|failure| Failure::engine(VOILE_FAILED, failure.to_string()))?;
    let Some(linux) = voile::linux_runtime(root) else {
        return Err(Failure::new(
            VOILE_FAILED,
            "The release laid carries no Linux engine.",
        ));
    };
    // Ours go in at once, so that what is told is what the next launch finds.
    let (keyboard, _held) = tokio::task::spawn_blocking(move || engine_patch::apply(&linux))
        .await
        .map_err(|_| Failure::new(VOILE_FAILED, "The engine's files were not checked."))?
        .map_err(|refused| {
            let error = game_runtime::launch::Error::EngineFiles(refused.into());
            Failure::engine(LAUNCH_FAILED, error.to_string())
        })?;
    out.emit(Event::EngineRepaired {
        voile: voile::installed_release(root),
        keyboard_layout: keyboard.name(),
        engine_files: files(root).unwrap_or_default(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Told;
    use std::fs;

    fn places(root: &Path) -> Places {
        Places {
            library: root.join("library"),
            helpers: root.join("SteamNative"),
            home: root.to_owned(),
        }
    }

    /// Nothing is laid over a development runtime, and a runtime that cannot
    /// be laid again is said, with what is in place left there.
    #[tokio::test]
    async fn a_repair_that_cannot_be_made_says_so_and_leaves_what_is_there() {
        let directory = tempfile::tempdir().unwrap();
        let places = places(directory.path());
        let root = places.library.join("Voile");
        fs::create_dir_all(root.join("current/bin")).unwrap();
        fs::write(root.join("current/bin/wine"), "wine").unwrap();
        let told = Told::default();
        let failure = repair(&root, &places, None, &told).await.unwrap_err();
        assert_eq!(failure.code, VOILE_FAILED);
        assert!(told.events().is_empty());

        fs::remove_dir_all(root.join("current")).unwrap();
        fs::create_dir_all(root.join("v1.0.0-1/linux/engine")).unwrap();
        fs::write(root.join("v1.0.0-1/linux/engine/wsd"), "kept").unwrap();
        let archive = directory.path().join("voile.tar.xz");
        fs::write(&archive, "not the release").unwrap();
        let failure = repair(&root, &places, Some(&archive), &told).await.unwrap_err();
        assert_eq!(failure.code, VOILE_FAILED);
        assert_eq!(failure.key.as_deref(), Some("backend.voile.damaged"));
        assert_eq!(fs::read(root.join("v1.0.0-1/linux/engine/wsd")).unwrap(), b"kept");
    }
}
