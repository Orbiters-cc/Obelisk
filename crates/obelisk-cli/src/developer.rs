//! The timeline generation's switch and commands (`docs/CLI.md`, "The timeline generation"). Feature `developer-timeline` only:
//! no release of this program has any of it.
use crate::{
    failure::{Failure, LAUNCH_FAILED, USAGE, VOILE_FAILED},
    places::Places,
};
use game_runtime::voile::{
    self,
    engine_patch::{
        self,
        developer::{self, Generation},
    },
};
use std::path::{Path, PathBuf};

/// Which engine files this process puts in place: `released` (as without it) or `timeline`.
const GENERATION: &str = "OBELISK_DEVELOPER_ENGINE";
/// The folder of that generation's files.
const FILES: &str = "OBELISK_DEVELOPER_ENGINE_FILES";

/// Reads the switch, before anything looks at an engine file.
pub fn select() -> Result<(), Failure> {
    let asked = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let generation = match asked(GENERATION).as_deref().map(|value| value.to_string_lossy().into_owned()).as_deref() {
        None | Some("released") => Generation::Released,
        Some("timeline") => Generation::Timeline,
        Some(other) => return Err(Failure::new(USAGE, format!("{GENERATION} is released or timeline, not {other}."))),
    };
    let folder = asked(FILES).map(PathBuf::from);
    if folder.as_ref().is_some_and(|folder| !folder.is_absolute()) {
        return Err(Failure::new(USAGE, format!("{FILES} is not an absolute path.")));
    }
    developer::select(generation, folder.as_deref()).map_err(|why| Failure::new(USAGE, format!("{why}.")))
}

fn tell(value: serde_json::Value) {
    println!("{value}");
}

/// `engine generation`: the generation asked for put in place as a launch puts it, and held to be whole.
pub async fn generation(places: &Places) -> Result<(), Failure> {
    places.open_library()?;
    let root = PathBuf::from(places.locations().voile);
    let linux = voile::linux_runtime(&root).ok_or_else(|| Failure::new(VOILE_FAILED, "No release with the Linux engine is installed."))?;
    let at = linux.clone();
    let (keyboard, _held) = tokio::task::spawn_blocking(move || engine_patch::apply(&at))
        .await
        .map_err(|_| Failure::new(VOILE_FAILED, "The engine's files were not checked."))?
        .map_err(|refused| {
            let error = game_runtime::launch::Error::EngineFiles(refused.into());
            Failure::engine(LAUNCH_FAILED, error.to_string())
        })?;
    if keyboard != engine_patch::Keyboard::Mac {
        return Err(Failure::new(VOILE_FAILED, "The engine's files are not this build's: something holds them as they are, or they are another release's."));
    }
    let files: Vec<_> = developer::digests(&linux).into_iter().map(|(path, sha256)| serde_json::json!({"path": path, "sha256": sha256})).collect();
    tell(serde_json::json!({
        "event": "engine_generation",
        "generation": developer::selected().name(),
        "voile": voile::installed_release(&root),
        "engine_files": files,
    }));
    Ok(())
}

/// `engine rehearse`: an owned Windows program on the engine, as a game starts there, followed until it ends.
pub async fn rehearse(places: &Places, install: PathBuf, program: PathBuf, percent: u32, log: PathBuf, arguments: Vec<String>) -> Result<(), Failure> {
    places.open_library()?;
    let locations = game_runtime::Locations::from(&places.locations());
    let failed = |error: game_runtime::launch::Error| Failure::engine(LAUNCH_FAILED, error.to_string());
    let started = tokio::task::spawn_blocking(move || {
        let (mut child, held) = game_runtime::wine::rehearsal::start(&locations, &install, Path::new(&program), &arguments, percent, &log)?;
        tell(serde_json::json!({"event": "rehearsal_running", "generation": developer::selected().name(), "pid": child.id()}));
        let status = child.wait();
        drop(held);
        Ok(status)
    });
    let status = started
        .await
        .map_err(|_| Failure::new(LAUNCH_FAILED, "The rehearsal was cut short."))?
        .map_err(failed)?
        .map_err(|error| Failure::new(LAUNCH_FAILED, error.to_string()))?;
    tell(serde_json::json!({"event": "rehearsal_exited", "exit_code": status.code()}));
    Ok(())
}
