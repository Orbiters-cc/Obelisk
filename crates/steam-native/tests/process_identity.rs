use obelisk_steam_native::native;
use std::process::{Command, Stdio};

#[test]
fn identity_helper_flushes_its_bounded_response_before_immediate_exit() {
    let expected = native::process_identity(std::process::id()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_obelisk-steam-host"))
        .args(["process-identity", &std::process::id().to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual: Option<game_runtime::supervisor::Identity> =
        serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(actual, Some(expected));
    let output = Command::new(env!("CARGO_BIN_EXE_obelisk-steam-host"))
        .args(["process-identity", "2147483647"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"null\n");
}

#[test]
fn native_supervisor_acknowledges_and_reports_preparation_failure_without_valve() {
    use game_runtime::{
        protocol::Credentials,
        steam::{Request, host::Config},
        supervisor::{Phase, Target},
    };
    use obelisk_core::transfer::CancelToken;
    use std::{
        path::PathBuf,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = Scratch(std::env::temp_dir().join(format!(
            "obelisk-ack-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&root.0).unwrap();
    let helper = PathBuf::from(env!("CARGO_BIN_EXE_obelisk-steam-host"));
    let credentials = Credentials {
        steam_id: 123,
        account_name: "fixture".into(),
        refresh_token: "fixture-no-steam-account".into(),
    };
    let client = game_runtime::steam::start(
        &helper,
        &credentials,
        Request {
            cloud: None,
            status: obelisk_core::OnlineStatus::default(),
            pin: None,
            id: String::new(),
            root: root.0.join("sessions"),
            config: Config {
                cloud_save_roots: vec![],
                app_id: 42,
                helpers: helper.parent().unwrap().into(),
                package: root.0.join("missing.zip"),
                cache: root.0.join("cache"),
                diagnostics: root.0.join("logs"),
                steam_api: Some(root.0.join("missing-api.dylib")),
                wine: None,
                home: root.0.clone(),
            },
            target: Target {
                executable: root.0.join("must-not-run"),
                arguments: vec![],
                working_directory: root.0.clone(),
                environment: Default::default(),
                log: None,
                installation: None,
                program: None,
            },
        },
        &CancelToken::new(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut after = None;
    loop {
        let snapshot = client.snapshot(after).unwrap();
        assert_eq!(snapshot.store.app_id, 42);
        assert_eq!(snapshot.account_id, 123);
        assert!(snapshot.game.is_none());
        if snapshot.finished() {
            assert_eq!(snapshot.phase, Phase::Failed);
            assert_eq!(
                snapshot.error.as_deref(),
                Some("backend.steam.runtime.unavailable")
            );
            break;
        }
        assert!(Instant::now() < deadline);
        after = Some(snapshot.revision);
    }
}

/// The real supervisor of a game on the Linux engine: accepted with the pin its
/// preparation took, as the same open lock, and refused without it.
#[test]
fn native_supervisor_of_the_linux_engine_holds_the_pin_it_was_started_with() {
    use game_runtime::{
        protocol::Credentials,
        steam::{Request, host::Config},
        supervisor::Target,
        voile::engine_patch,
        wine,
    };
    use obelisk_core::transfer::CancelToken;
    use std::path::PathBuf;
    let root = std::env::temp_dir().join(format!("obelisk-pin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let settings = || {
        serde_json::from_value(serde_json::json!({
            "engine": "Linux", "graphics": "Dxmt", "options": {"ray_tracing": true},
            "executable": null, "bootstrapper": null, "profile_arguments": [], "arguments": [],
            "variables": {}, "dll_overrides": {}, "performance_hud": false,
        }))
        .unwrap()
    };
    let release = root.join("Voile/v9.0.0-1");
    let runtime = release.join("linux");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(release.join("runtime.json"), r#"{"version":"v9.0.0-1"}"#).unwrap();
    let helper = PathBuf::from(env!("CARGO_BIN_EXE_obelisk-steam-host"));
    let start = |name: &str, pin| {
        let credentials = Credentials {
            steam_id: 123,
            account_name: "fixture".into(),
            refresh_token: "fixture-no-steam-account".into(),
        };
        game_runtime::steam::start(
            &helper,
            &credentials,
            Request {
                cloud: None,
                status: obelisk_core::OnlineStatus::default(),
                pin,
                id: String::new(),
                root: root.join(name),
                config: Config {
                    cloud_save_roots: vec![],
                    app_id: 42,
                    helpers: helper.parent().unwrap().into(),
                    package: root.join("missing.zip"),
                    cache: root.join("cache"),
                    diagnostics: root.join("logs"),
                    steam_api: None,
                    wine: Some(wine::Config {
                        runtime: runtime.clone(),
                        prefix: root.join("prefix"),
                        log: root.join("wine.log"),
                        settings: settings(),
                    }),
                    home: root.clone(),
                },
                target: Target {
                    executable: root.join("must-not-run"),
                    arguments: vec![],
                    working_directory: root.clone(),
                    environment: Default::default(),
                    log: None,
                    installation: None,
                    program: None,
                },
            },
            &CancelToken::new(),
        )
    };
    let Err(refused) = start("without", None) else {
        panic!("a launch without its pin was accepted")
    };
    assert_ne!(refused.to_string(), "");
    let (_, pin) = engine_patch::apply(&runtime).unwrap();
    let accepted = start("with", Some(pin)).unwrap();
    // It was accepted (its preparation then fails: there is no Steam here).
    let mut record = accepted.snapshot(None).unwrap();
    assert_eq!(record.store.app_id, 42);
    while !record.finished() {
        record = accepted.snapshot(Some(record.revision)).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}
