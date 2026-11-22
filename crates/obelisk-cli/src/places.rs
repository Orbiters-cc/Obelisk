//! Where this program keeps everything, apart from the Obelisk app's own
//! library: its library directory, the Steam helpers beside its executable,
//! and its cache of Valve's Steam components.
use crate::{
    events,
    failure::{Failure, HELPERS_MISSING, LIBRARY_UNAVAILABLE},
};
use obelisk_bridge::{ObeliskLocations, obelisk_locations};
use std::path::{Path, PathBuf};

/// The name its library and its cache go by, and its Keychain items start
/// with.
pub const IDENTIFIER: &str = "cc.orbiters.obelisk-cli";
/// The folder of the Steam helpers, beside the executable.
const HELPERS: &str = "SteamNative";
/// The Steam host every supervisor runs as.
const HOST: &str = game_runtime::package::HOST;
/// The helpers a launch needs beside the host: the library the host loads,
/// then the Intel one that readies D3DMetal for the Linux engine's games.
const BESIDE_HOST: [&str; 2] = [
    "libobelisk_steam_discovery_adapter.dylib",
    "obelisk-steam-sdk-x86_64",
];
/// The Intel helper app that tells whether Rosetta is installed, beside the
/// helpers' folder.
const ROSETTA_HELPER: &str = "Obelisk Rosetta.app/Contents/MacOS/ObeliskRosetta";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub library: PathBuf,
    pub helpers: PathBuf,
    pub home: PathBuf,
}

impl Places {
    /// The places of an invocation: the `library` and `helpers` it names,
    /// else the library in `home` and the helpers beside `executable`. A
    /// relative path is taken from `current`, the working directory.
    pub fn resolve(
        library: Option<PathBuf>,
        helpers: Option<PathBuf>,
        home: &Path,
        executable: &Path,
        current: &Path,
    ) -> Self {
        let library = library.map_or_else(
            || home.join("Library/Application Support").join(IDENTIFIER),
            |library| current.join(library),
        );
        let helpers = helpers.map_or_else(
            || executable.parent().unwrap_or(current).join(HELPERS),
            |helpers| current.join(helpers),
        );
        Self {
            library,
            helpers,
            home: home.to_owned(),
        }
    }

    /// The places of this process.
    pub fn of_process(library: Option<PathBuf>, helpers: Option<PathBuf>) -> Result<Self, Failure> {
        let unavailable = |what: &str| Failure::new(LIBRARY_UNAVAILABLE, what);
        let home = std::env::home_dir()
            .filter(|home| home.is_absolute())
            .ok_or_else(|| unavailable("The home directory is unknown."))?;
        let current = std::env::current_dir()
            .map_err(|_| unavailable("The working directory cannot be read."))?;
        // A link to the program still finds the helpers beside the program.
        let executable = std::env::current_exe()
            .and_then(|executable| executable.canonicalize())
            .map_err(|_| unavailable("This program's own path cannot be read."))?;
        Ok(Self::resolve(
            library,
            helpers,
            &home,
            &executable,
            &current,
        ))
    }

    /// Every path the engine uses, as Obelisk names them: the app's own,
    /// but for the helpers, which sit beside this program, and Valve's Steam
    /// components, which are kept in this program's cache.
    pub fn locations(&self) -> ObeliskLocations {
        let path = |path: &Path| path.to_string_lossy().into_owned();
        let cache = self
            .home
            .join("Library/Caches")
            .join(IDENTIFIER)
            .join("SteamRuntime");
        ObeliskLocations {
            helper: path(&self.helpers.join(HOST)),
            helpers: path(&self.helpers),
            steam_runtime_package: path(&game_runtime::valve::payload(&cache)),
            steam_runtime_cache: path(&cache),
            ..obelisk_locations(path(&self.library), String::new(), path(&self.home))
        }
    }

    /// The file that keeps the Installation receipts of the Steam account
    /// `steam_id`, named as the app names it.
    pub fn receipts(&self, steam_id: u64) -> PathBuf {
        self.library
            .join(format!("steam-installations-{steam_id}.json"))
    }

    /// The Game session records of Steam's supervisors.
    pub fn steam_sessions(&self) -> PathBuf {
        PathBuf::from(self.locations().steam_sessions)
    }

    /// Where the helpers stand.
    pub fn helpers(&self) -> events::Helpers {
        let missing: Vec<String> = std::iter::once(HOST)
            .chain(BESIDE_HOST)
            .filter(|name| !self.helpers.join(name).is_file())
            .map(str::to_owned)
            .collect();
        let rosetta = self
            .helpers
            .parent()
            .is_some_and(|beside| beside.join(ROSETTA_HELPER).is_file());
        events::Helpers {
            directory: self.helpers.to_string_lossy().into_owned(),
            found: missing.is_empty(),
            missing,
            rosetta_helper: rosetta,
        }
    }

    /// Makes the library directory, and refuses one that is not a folder.
    pub fn open_library(&self) -> Result<(), Failure> {
        std::fs::create_dir_all(&self.library).map_err(|error| {
            Failure::new(
                LIBRARY_UNAVAILABLE,
                format!(
                    "The library {} cannot be used: {error}.",
                    self.library.display()
                ),
            )
        })
    }

    /// Refuses a command that reaches Game sessions without the Steam host:
    /// their recovery asks it about every recorded process, and would wait
    /// for it forever.
    pub fn require_host(&self) -> Result<(), Failure> {
        if self.helpers.join(HOST).is_file() {
            return Ok(());
        }
        Err(Failure::new(
            HELPERS_MISSING,
            format!(
                "{HOST} is not in {}: name its folder with --helpers.",
                self.helpers.display()
            ),
        ))
    }

    /// Refuses a launch that misses a helper.
    pub fn require_helpers(&self) -> Result<(), Failure> {
        let helpers = self.helpers();
        if helpers.found {
            return Ok(());
        }
        Err(Failure::new(
            HELPERS_MISSING,
            format!(
                "{} missing in {}: name the helpers' folder with --helpers.",
                helpers.missing.join(", "),
                helpers.directory
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(library: Option<&str>, helpers: Option<&str>) -> Places {
        Places::resolve(
            library.map(PathBuf::from),
            helpers.map(PathBuf::from),
            Path::new("/Users/player"),
            Path::new("/opt/obelisk-cli/obelisk-cli"),
            Path::new("/work"),
        )
    }

    #[test]
    fn the_library_is_this_program_s_own_and_the_helpers_sit_beside_it() {
        let places = places(None, None);
        assert_eq!(
            places.library,
            Path::new("/Users/player/Library/Application Support/cc.orbiters.obelisk-cli")
        );
        assert_eq!(places.helpers, Path::new("/opt/obelisk-cli/SteamNative"));
        assert!(
            !places.library.to_string_lossy().contains("com.zagrios"),
            "never the Obelisk app's library"
        );
    }

    #[test]
    fn a_named_library_or_helpers_folder_is_taken_from_the_working_directory() {
        let named = places(Some("library"), Some("/elsewhere/helpers"));
        assert_eq!(named.library, Path::new("/work/library"));
        assert_eq!(named.helpers, Path::new("/elsewhere/helpers"));
    }

    #[test]
    fn every_engine_path_stays_in_the_library_the_helpers_folder_or_this_program_s_cache() {
        let locations = places(Some("/library"), None).locations();
        assert_eq!(locations.library, "/library");
        assert_eq!(locations.home, "/Users/player");
        assert_eq!(locations.steam_sessions, "/library/SteamSessions");
        assert_eq!(locations.cloud_archive, "/library/SteamCloud");
        assert_eq!(locations.voile, "/library/Voile");
        assert_eq!(locations.wine_prefixes, "/library/Prefixes");
        assert_eq!(locations.helpers, "/opt/obelisk-cli/SteamNative");
        assert_eq!(
            locations.helper,
            "/opt/obelisk-cli/SteamNative/obelisk-steam-host"
        );
        assert_eq!(
            locations.steam_runtime_cache,
            "/Users/player/Library/Caches/cc.orbiters.obelisk-cli/SteamRuntime"
        );
        assert_eq!(
            locations.steam_runtime_package,
            format!(
                "/Users/player/Library/Caches/cc.orbiters.obelisk-cli/SteamRuntime/valve-{}.zip",
                game_runtime::package::manifest().version
            )
        );
        for path in [
            &locations.cloud_archive,
            &locations.steam_sessions,
            &locations.epic_sessions,
            &locations.gog_sessions,
            &locations.helper,
            &locations.helpers,
            &locations.steam_runtime_package,
            &locations.steam_runtime_cache,
            &locations.voile,
            &locations.wine_prefixes,
        ] {
            assert!(!path.contains("com.zagrios"), "{path}");
        }
    }

    #[test]
    fn receipts_are_kept_per_account_under_the_name_the_app_gives_them() {
        assert_eq!(
            places(Some("/library"), None).receipts(76_561_197_960_265_729),
            Path::new("/library/steam-installations-76561197960265729.json")
        );
    }

    #[test]
    fn the_helpers_found_and_missing_are_told_and_a_launch_needs_them_all() {
        let root = tempfile::tempdir().unwrap();
        let helpers = root.path().join("SteamNative");
        let places = Places {
            library: root.path().join("library"),
            helpers: helpers.clone(),
            home: root.path().to_owned(),
        };
        assert_eq!(places.helpers().missing.len(), 3);
        assert_eq!(places.require_host().unwrap_err().code, HELPERS_MISSING);

        std::fs::create_dir_all(&helpers).unwrap();
        std::fs::write(helpers.join(HOST), "").unwrap();
        assert!(places.require_host().is_ok());
        let partial = places.helpers();
        assert!(!partial.found);
        assert_eq!(partial.missing, BESIDE_HOST);
        assert!(!partial.rosetta_helper);
        assert_eq!(places.require_helpers().unwrap_err().code, HELPERS_MISSING);

        for name in BESIDE_HOST {
            std::fs::write(helpers.join(name), "").unwrap();
        }
        let rosetta = root.path().join(ROSETTA_HELPER);
        std::fs::create_dir_all(rosetta.parent().unwrap()).unwrap();
        std::fs::write(rosetta, "").unwrap();
        let all = places.helpers();
        assert!(all.found && all.missing.is_empty() && all.rosetta_helper);
        assert!(places.require_helpers().is_ok());
    }

    #[test]
    fn a_library_that_is_a_file_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("library");
        std::fs::write(&file, "").unwrap();
        let places = Places {
            library: file,
            helpers: root.path().to_owned(),
            home: root.path().to_owned(),
        };
        assert_eq!(places.open_library().unwrap_err().code, LIBRARY_UNAVAILABLE);
    }
}
