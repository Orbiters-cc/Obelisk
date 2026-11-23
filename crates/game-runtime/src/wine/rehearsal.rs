//! A developer's rehearsal: an owned Windows program on the Linux engine as a launch starts a game there (the engine's files
//! proven and pinned, the prefix readied, the launcher in an environment of the launch's own alone, its output in a log), without
//! a storefront, an account or a supervisor. Feature `developer-timeline` only.
use super::{Additions, Config, Engine, Furnisher, Game, Graphics, Prerequisite};
use crate::{Locations, launch::Error};
use obelisk_core::launch::LaunchTarget;
use std::{
    collections::HashMap,
    path::Path,
    process::{Child, Command, Stdio},
};

struct NoStore;

impl Furnisher for NoStore {
    fn additions(&self) -> Additions {
        Additions { helper: None, passed: &[], steam_client: false }
    }
    fn furnish_prefix(&self, _: &Config) -> Result<(), Error> {
        Ok(())
    }
    fn prerequisites_to_run(&self) -> Result<Vec<Prerequisite>, Error> {
        Ok(Vec::new())
    }
}

/// Starts `program` of `install`, its output in `log`; the launch holds the engine's files until the program it returns ends
/// and is dropped with what holds them.
pub fn start(
    locations: &Locations,
    install: &Path,
    program: &Path,
    arguments: &[String],
    render_percent: u32,
    log: &Path,
) -> Result<(Child, impl Sized + use<>), Error> {
    let settings = crate::launch_settings::Settings {
        engine: Engine::Linux,
        graphics: Graphics::Dxmt,
        options: super::GraphicsOptions::default(),
        executable: None,
        bootstrapper: None,
        profile_arguments: Vec::new(),
        arguments: Vec::new(),
        variables: std::collections::BTreeMap::new(),
        dll_overrides: std::collections::BTreeMap::new(),
        performance_hud: false,
        render_percent,
        game_mode: true,
        locale: None,
    };
    let admitted = super::admit(locations)?;
    let runtime = crate::voile::linux_runtime(&locations.voile).ok_or(Error::WineUnavailable)?;
    let config = Config {
        runtime,
        prefix: Engine::Linux.prefix(&locations.wine_prefixes, install)?,
        log: log.to_owned(),
        settings,
    };
    let adapter = config.adapter();
    let pin = adapter.ready(&config, locations, &NoStore, admitted)?;
    let launched = LaunchTarget {
        executable: program.to_string_lossy().into_owned(),
        arguments: arguments.to_vec(),
        working_directory: install.to_string_lossy().into_owned(),
        is_application_bundle: false,
    };
    let program = super::program(install, &launched)?;
    let game = Game { install, title: "Rehearsal" };
    let mut environment: HashMap<String, String> = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR"]
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?)))
        .collect();
    environment.extend(crate::metal_hud::environment(false).map(|(name, value)| (name.to_owned(), value.to_owned())));
    let target = adapter.target(&config, &game, &launched, program, NoStore.additions(), environment)?;
    let output = crate::diagnostics::open_log(log)?;
    let child = Command::new(&target.executable)
        .args(&target.arguments)
        .current_dir(&target.working_directory)
        .env_clear()
        .envs(&target.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(output)
        .spawn()?;
    Ok((child, pin))
}
