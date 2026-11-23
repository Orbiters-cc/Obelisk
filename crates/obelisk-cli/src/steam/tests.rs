use super::*;
use crate::{
    credentials::{Memory, gordon},
    events::Told,
    failure::{HELPERS_MISSING, NOT_SIGNED_IN},
};
use std::path::PathBuf;

/// The places of a test, under `root`: no helper is there.
fn places(root: &Path) -> Places {
    Places {
        library: root.join("library"),
        helpers: root.join("SteamNative"),
        home: root.to_owned(),
    }
}

/// Runs `command` on a current-thread runtime.
fn run<T>(command: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(command)
}

#[test]
fn the_phone_s_approval_comes_before_a_code_unless_one_is_asked_for() {
    use SteamGuardKind as Kind;
    let offered = [Kind::EmailCode, Kind::DeviceCode, Kind::DeviceConfirmation];
    assert_eq!(
        choose_guard(&offered, None).unwrap(),
        Some(Guard::DeviceConfirmation)
    );
    assert_eq!(
        choose_guard(&[Kind::EmailCode, Kind::DeviceCode], None).unwrap(),
        Some(Guard::DeviceCode)
    );
    assert_eq!(
        choose_guard(&[Kind::EmailConfirmation, Kind::EmailCode], None).unwrap(),
        Some(Guard::EmailCode)
    );
    assert_eq!(
        choose_guard(&offered, Some(Guard::DeviceCode)).unwrap(),
        Some(Guard::DeviceCode)
    );
    assert_eq!(choose_guard(&[Kind::None], None).unwrap(), None);
    assert_eq!(choose_guard(&[], None).unwrap(), None);
    let refused = choose_guard(&[Kind::DeviceConfirmation], Some(Guard::EmailCode)).unwrap_err();
    assert_eq!(refused.code, GUARD_UNSUPPORTED);
    assert!(refused.message.contains("device_confirmation"));
    assert_eq!(
        choose_guard(&[Kind::MachineToken, Kind::Unknown], None)
            .unwrap_err()
            .code,
        GUARD_UNSUPPORTED
    );
    assert!(needs_code(Guard::DeviceCode) && needs_code(Guard::EmailCode));
    assert!(!needs_code(Guard::DeviceConfirmation) && !needs_code(Guard::EmailConfirmation));
}

#[test]
fn a_game_s_process_is_read_from_its_supervisor_s_record_and_nothing_else() {
    let sessions = tempfile::tempdir().unwrap();
    let id = "6f1c8a52-0c0e-4a59-9d4e-0d6a6a0f2b11";
    let record =
        |game: &str| format!(r#"{{"version":2,"id":"{id}","phase":"Running","game":{game}}}"#);
    let file = sessions.path().join(format!("{id}.v2.json"));
    std::fs::write(&file, record(r#"{"pid":4242,"uid":501}"#)).unwrap();
    assert_eq!(recorded_pid(sessions.path(), id), Some(4242));
    std::fs::write(&file, record("null")).unwrap();
    assert_eq!(recorded_pid(sessions.path(), id), None);
    std::fs::write(&file, "not json").unwrap();
    assert_eq!(recorded_pid(sessions.path(), id), None);
    assert_eq!(recorded_pid(sessions.path(), "missing"), None);
    std::fs::write(
        sessions.path().join("session.v2.json"),
        record(r#"{"pid":1}"#),
    )
    .unwrap();
    assert_eq!(
        recorded_pid(sessions.path(), "session"),
        None,
        "only a session's own record is read"
    );
    assert_eq!(recorded_pid(sessions.path(), "../escape"), None);
}

/// A receipts file as the app's installations write it, of `folders`.
fn receipts(file: &Path, folders: &[(&Path, u32, &str, bool)]) {
    let receipts: Vec<_> = folders
        .iter()
        .map(|(directory, app_id, name, needs_verification)| {
            let mut receipt = serde_json::json!({
                "appId": app_id, "language": "english", "name": name,
                "directory": directory, "platform": "windows", "totalBytes": 1500,
            });
            if *needs_verification {
                receipt["needsVerification"] = true.into();
            }
            receipt
        })
        .collect();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, serde_json::to_vec(&receipts).unwrap()).unwrap();
}

#[test]
fn installed_lists_the_account_s_receipts_whose_folder_is_there_without_reaching_steam() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let here = root.path().join("Games/steam-438100-windows");
    std::fs::create_dir_all(&here).unwrap();
    let gone = root.path().join("Games/steam-620-windows");
    receipts(
        &places.receipts(gordon().steam_id),
        &[
            (&here, 438100, "VRChat", true),
            (&gone, 620, "Portal 2", false),
        ],
    );
    let (store, out) = (Memory::holding(gordon()), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    steam.installed().unwrap();
    assert_eq!(
        out.events(),
        [
            Event::Installation {
                app_id: 438100,
                name: "VRChat".into(),
                directory: here.to_string_lossy().into_owned(),
                platform: "windows",
                bytes: 1500,
                needs_verification: true,
            },
            Event::Done { count: 1 }
        ]
    );
}

#[test]
fn installed_lists_nothing_signed_out_or_before_any_install() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    for store in [Memory::default(), Memory::holding(gordon())] {
        let out = Told::default();
        let steam = Steam {
            places: &places,
            store: &store,
            out: &out,
        };
        steam.installed().unwrap();
        assert_eq!(out.events(), [Event::Done { count: 0 }]);
    }
    std::fs::create_dir_all(&places.library).unwrap();
    std::fs::write(places.receipts(gordon().steam_id), "not receipts").unwrap();
    let (store, out) = (Memory::holding(gordon()), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    assert_eq!(steam.installed().unwrap_err().code, INTERNAL);
    assert!(out.events().is_empty());
}

#[test]
fn a_command_that_needs_an_account_fails_signed_out_before_it_makes_or_reaches_anything() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let (store, out) = (Memory::default(), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    assert_eq!(
        run(steam.play(438100, None, vec!["--no-vr".into()], None, true))
            .unwrap_err()
            .code,
        NOT_SIGNED_IN
    );
    assert_eq!(
        run(steam.install(438100, None)).unwrap_err().code,
        NOT_SIGNED_IN
    );
    assert!(out.events().is_empty());
    assert!(!places.library.exists());
}

#[test]
fn a_launch_without_its_helpers_fails_before_it_reaches_steam() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let (store, out) = (Memory::holding(gordon()), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    assert_eq!(
        run(steam.play(438100, None, Vec::new(), Some(58), false)).unwrap_err().code,
        HELPERS_MISSING
    );
    assert_eq!(run(steam.running()).unwrap_err().code, HELPERS_MISSING);
    assert_eq!(
        run(steam.quit(438100, false, None, None)).unwrap_err().code,
        HELPERS_MISSING
    );
    assert!(out.events().is_empty());
}

#[test]
fn logout_forgets_the_account_and_says_whether_one_was_signed_in() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let (store, out) = (Memory::holding(gordon()), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    run(steam.logout()).unwrap();
    run(steam.logout()).unwrap();
    assert!(store.saved.lock().unwrap().is_none());
    assert_eq!(
        out.events(),
        [
            Event::SignedOut {
                was_signed_in: true
            },
            Event::SignedOut {
                was_signed_in: false
            }
        ]
    );
}

#[test]
fn the_environment_s_locale_names_the_language_a_game_installs_in() {
    assert_eq!(steam_language("fr_FR.UTF-8".into()), "french");
    assert_eq!(steam_language("en".into()), "english");
    let _: PathBuf = places(Path::new("/root")).steam_sessions();
}

/// Sixth verification, the restart finding: the account a launch was asked for
/// is checked against the credential it starts with, read once, before
/// anything is started or reached; a later change of the saved account starts
/// nothing for another one.
#[test]
fn a_launch_for_one_account_never_starts_for_another() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let (store, out) = (Memory::holding(gordon()), Told::default());
    let steam = Steam {
        places: &places,
        store: &store,
        out: &out,
    };
    let play = |steam_id| run(steam.play(438100, steam_id, Vec::new(), None, true)).unwrap_err().code;
    assert_eq!(play(Some(gordon().steam_id + 1)), crate::failure::OTHER_ACCOUNT);
    assert!(out.events().is_empty() && !places.library.exists());
    // Its own goes on (as far as this test's missing helpers let it).
    assert_eq!(play(Some(gordon().steam_id)), HELPERS_MISSING);
    assert_eq!(play(None), HELPERS_MISSING);
    // The saved account changes as the restart's last look is made.
    let mut other = gordon();
    other.steam_id += 1;
    store.save(&other).unwrap();
    assert_eq!(play(Some(gordon().steam_id)), crate::failure::OTHER_ACCOUNT);
}
