//! `obelisk-cli`: Obelisk's engine without its window. Another program runs
//! it to sign in to Steam, install a Windows game, start it, learn when it
//! runs and when it ended, and stop it. Each command tells what happens as
//! JSON Lines on standard output; docs/CLI.md is the contract.
//!
//! It calls the same Rust the app's Swift calls, `obelisk-bridge`, in the
//! same order, and keeps a library, a Steam session and a cache of its own.
mod arguments;
mod credentials;
mod events;
mod failure;
mod follow;
mod places;
mod steam;

use arguments::Command;
use credentials::{CredentialStore, Keychain};
use events::{Event, Sink, Stdout};
use failure::{Failure, INTERNAL, SETTINGS_FAILED};
use places::Places;
use std::path::Path;

/// This program's version, which its releases are tagged with.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let out = Stdout;
    let status = match run(&out) {
        Ok(()) => 0,
        Err(failure) => {
            out.emit(Event::from(&failure));
            failure.status()
        }
    };
    // The engine's own threads follow Game sessions for as long as they
    // last: the command is done, and the supervisor owns the game.
    std::process::exit(status);
}

fn run(out: &dyn Sink) -> Result<(), Failure> {
    let invocation = arguments::parse(std::env::args().skip(1))?;
    match invocation.command {
        Command::Help => {
            eprint!("{}", arguments::HELP);
            return Ok(());
        }
        Command::Version => {
            out.emit(Event::Version { version: VERSION });
            return Ok(());
        }
        _ => {}
    }
    let places = Places::of_process(invocation.library, invocation.helpers)?;
    // Nobody is there to answer a Keychain question: a session this program
    // may not read fails its command instead of waiting for one.
    let _quiet = security_framework::os::macos::keychain::SecKeychain::disable_user_interaction();
    let store = Keychain::of(&places.library);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Failure::new(INTERNAL, format!("No runtime could start: {error}.")))?;
    runtime.block_on(dispatch(invocation.command, &places, &store, out))
}

async fn dispatch(
    command: Command,
    places: &Places,
    store: &dyn CredentialStore,
    out: &dyn Sink,
) -> Result<(), Failure> {
    let steam = steam::Steam { places, store, out };
    match command {
        Command::Help | Command::Version => Ok(()),
        Command::Status => {
            out.emit(status(places, store));
            Ok(())
        }
        Command::Login(login) => steam.login(login).await,
        Command::Logout => steam.logout().await,
        Command::Installed => steam.installed(),
        Command::Install { app_id, language } => steam.install(app_id, language).await,
        Command::Play { app_id, arguments } => steam.play(app_id, arguments).await,
        Command::Running => steam.running().await,
        Command::Quit {
            app_id,
            force,
            wait,
        } => steam.quit(app_id, force, wait).await,
        Command::Prereleases(take) => prereleases(places, take).map(|event| out.emit(event)),
    }
}

/// `status`: what is in place, read without reaching Steam or changing
/// anything.
fn status(places: &Places, store: &dyn CredentialStore) -> Event {
    let (account, keychain) = match store.load() {
        Ok(saved) => (saved.map(|saved| saved.account()), "ok"),
        Err(_) => (None, "unavailable"),
    };
    let locations = places.locations();
    Event::Status {
        version: VERSION,
        library: places.library.to_string_lossy().into_owned(),
        account,
        keychain,
        voile: events::Voile {
            prereleases: game_runtime::voile::takes_prereleases(Path::new(&locations.voile)),
            keyboard_layout: game_runtime::voile::keyboard(Path::new(&locations.voile))
                .map(game_runtime::voile::engine_patch::Keyboard::name),
            installed: obelisk_bridge::installed_voile_release(locations),
        },
        helpers: places.helpers(),
    }
}

/// `settings prereleases`: whether Windows games run in Voile's
/// pre-releases too, as the app's beta releases setting chooses.
fn prereleases(places: &Places, take: bool) -> Result<Event, Failure> {
    places.open_library()?;
    let locations = places.locations();
    let voile = locations.voile.clone();
    if !obelisk_bridge::take_voile_prereleases(locations, take) {
        return Err(Failure::new(
            SETTINGS_FAILED,
            format!("The choice could not be kept in {voile}."),
        ));
    }
    Ok(Event::Settings {
        prereleases: game_runtime::voile::takes_prereleases(Path::new(&voile)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use credentials::{Memory, gordon};

    fn places(root: &Path) -> Places {
        Places {
            library: root.join("library"),
            helpers: root.join("SteamNative"),
            home: root.to_owned(),
        }
    }

    #[test]
    fn status_tells_the_account_without_its_session_and_what_is_in_place() {
        let root = tempfile::tempdir().unwrap();
        let places = places(root.path());
        let line = status(&places, &Memory::holding(gordon())).line();
        let told: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(told["event"], "status");
        assert_eq!(told["version"], VERSION);
        assert_eq!(told["account"]["steam_id"], "76561197960265729");
        assert_eq!(told["account"]["account_name"], "gordon");
        assert_eq!(told["keychain"], "ok");
        assert_eq!(told["voile"]["installed"], serde_json::Value::Null);
        assert_eq!(told["voile"]["prereleases"], false);
        assert_eq!(told["voile"]["keyboard_layout"], serde_json::Value::Null);
        assert_eq!(told["helpers"]["found"], false);
        assert!(!line.contains("secret-refresh-token"));
        assert!(
            !places.library.exists(),
            "status makes nothing, not even the library"
        );

        let signed_out = status(&places, &Memory::default()).line();
        let told: serde_json::Value = serde_json::from_str(&signed_out).unwrap();
        assert_eq!(told["account"], serde_json::Value::Null);
    }

    #[test]
    fn status_tells_a_keychain_that_did_not_answer_apart_from_no_account() {
        let root = tempfile::tempdir().unwrap();
        let store = Memory::holding(gordon());
        store
            .broken
            .store(true, std::sync::atomic::Ordering::Release);
        let Event::Status {
            account, keychain, ..
        } = status(&places(root.path()), &store)
        else {
            panic!("status tells a status event");
        };
        assert_eq!(account, None);
        assert_eq!(keychain, "unavailable");
    }

    #[test]
    fn taking_prereleases_is_kept_in_the_library_and_shows_in_the_status() {
        let root = tempfile::tempdir().unwrap();
        let places = places(root.path());
        assert_eq!(
            prereleases(&places, true).unwrap(),
            Event::Settings { prereleases: true }
        );
        assert!(places.library.join("Voile/prereleases").is_file());
        let Event::Status { voile, .. } = status(&places, &Memory::default()) else {
            panic!("status tells a status event");
        };
        assert!(voile.prereleases);
        assert_eq!(
            prereleases(&places, false).unwrap(),
            Event::Settings { prereleases: false }
        );
        assert!(!places.library.join("Voile/prereleases").exists());
    }
}
