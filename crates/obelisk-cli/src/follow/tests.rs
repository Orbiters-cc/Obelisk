use super::*;
use obelisk_bridge::{
    CloudPhase, CloudQuestion, CloudStatus, CloudSync, GameOpening, GamePlayed, InstallationGate,
    QueuedTransfer, RosettaPromptState, RuntimeInstallState, VoileUpdateOffer,
};
use std::time::Duration;

const VRCHAT: u32 = 438100;
const GORDON: u64 = 76_561_197_960_265_729;

/// The installations of an account with nothing under way.
fn idle() -> InstallationsState {
    let open = || GameOpening::Open { except: Vec::new() };
    InstallationsState {
        revision: 1,
        installations: Vec::new(),
        transfer: None,
        transfer_game: None,
        downloading: false,
        launching: false,
        verifying_before_launch: false,
        launch_game: None,
        uninstall_game: None,
        resumable: None,
        resumable_build: None,
        incomplete: Vec::new(),
        report: None,
        warning: None,
        gate: InstallationGate {
            modify: open(),
            uninstall: open(),
            launch: open(),
            maintain: open(),
        },
        cloud_sync: None,
        queued: Vec::new(),
    }
}

/// VRChat's Windows installation.
fn vrchat() -> Installation {
    Installation {
        id: "438100-windows".into(),
        game: "438100".into(),
        name: "VRChat".into(),
        directory: "/library/Games/Steam/steam-438100-windows".into(),
        platform: GamePlatform::Windows,
        total_bytes: 1_500,
        needs_verification: false,
        update_available: false,
    }
}

/// VRChat's Transfer in `state`, `done` bytes of 1 000 in.
fn transfer(state: TransferState, done: u64, message: &str) -> GameTransfer {
    GameTransfer {
        game: "438100".into(),
        state,
        completed_bytes: done,
        total_bytes: 1_000,
        network_bytes: done,
        content_bytes: done,
        current_file: String::new(),
        message: message.into(),
    }
}

/// The installations while VRChat's Transfer is as `transfer` says.
fn transferring(transfer: GameTransfer) -> InstallationsState {
    let settled = matches!(
        transfer.state,
        TransferState::Complete | TransferState::Cancelled | TransferState::Failed
    );
    InstallationsState {
        downloading: !settled,
        transfer_game: Some("438100".into()),
        transfer: Some(transfer),
        ..idle()
    }
}

fn told(steps: &[InstallStep]) -> Vec<String> {
    steps
        .iter()
        .map(|step| match step {
            InstallStep::Tell(event) => event.line(),
            InstallStep::End(Ok(installed)) => format!("installed {}", installed.id),
            InstallStep::End(Err(failure)) => format!("failed {}", failure.code),
        })
        .collect()
}

#[test]
fn an_install_tells_each_runtime_then_the_game_s_files_then_its_installation() {
    let follow = Install::new(VRCHAT);
    let start = Instant::now();
    let at = |milliseconds| start + Duration::from_millis(milliseconds);
    let mut lines = Vec::new();
    for (state, when) in [
        (
            transfer(TransferState::Preparing, 0, "backend.transfer.preparing"),
            0,
        ),
        (
            transfer(TransferState::Downloading, 100, INSTALLING_STEAM_COMPONENTS),
            10,
        ),
        (
            transfer(TransferState::Downloading, 900, INSTALLING_STEAM_COMPONENTS),
            20,
        ),
        (
            transfer(TransferState::Downloading, 250, INSTALLING_VOILE),
            30,
        ),
        (transfer(TransferState::Downloading, 0, ""), 40),
        (transfer(TransferState::Downloading, 400, ""), 100),
        (transfer(TransferState::Downloading, 500, ""), 300),
        (transfer(TransferState::Verifying, 1_000, ""), 310),
    ] {
        lines.extend(told(&follow.observe(&transferring(state), at(when))));
    }
    assert_eq!(
        lines,
        [
            r#"{"event":"progress","app_id":438100,"phase":"preparing","bytes_done":0,"bytes_total":1000,"fraction":0.0}"#,
            r#"{"event":"progress","app_id":438100,"phase":"steam_components","bytes_done":100,"bytes_total":1000,"fraction":0.1}"#,
            r#"{"event":"progress","app_id":438100,"phase":"voile","bytes_done":250,"bytes_total":1000,"fraction":0.25}"#,
            r#"{"event":"progress","app_id":438100,"phase":"downloading","bytes_done":0,"bytes_total":1000,"fraction":0.0}"#,
            r#"{"event":"progress","app_id":438100,"phase":"downloading","bytes_done":500,"bytes_total":1000,"fraction":0.5}"#,
            r#"{"event":"progress","app_id":438100,"phase":"verifying","bytes_done":1000,"bytes_total":1000,"fraction":1.0}"#,
        ],
        "a phase tells its first progress, then one every quarter of a second"
    );
    let complete = InstallationsState {
        installations: vec![vrchat()],
        ..transferring(transfer(TransferState::Complete, 1_000, ""))
    };
    assert_eq!(
        told(&follow.observe(&complete, at(400))),
        ["installed 438100-windows"]
    );
}

#[test]
fn an_install_waits_its_turn_in_the_queue_and_tells_nothing_meanwhile() {
    let follow = Install::new(VRCHAT);
    let waiting = InstallationsState {
        queued: vec![QueuedTransfer {
            game: "438100".into(),
            build: Some(GamePlatform::Windows),
        }],
        // Another game's Transfer runs.
        downloading: true,
        transfer_game: Some("620".into()),
        transfer: Some(GameTransfer {
            game: "620".into(),
            ..transfer(TransferState::Downloading, 10, "")
        }),
        ..idle()
    };
    assert!(follow.observe(&waiting, Instant::now()).is_empty());
}

#[test]
fn an_install_that_never_joined_the_queue_is_busy_or_tells_why() {
    let follow = Install::new(VRCHAT);
    assert_eq!(
        told(&follow.observe(&idle(), Instant::now())),
        ["failed busy"]
    );
    let folder = InstallationsState {
        report: Some(InstallationReport::GamesFolderUnavailable {
            path: "/Volumes/Games".into(),
        }),
        ..idle()
    };
    let steps = follow.observe(&folder, Instant::now());
    let [InstallStep::End(Err(failure))] = &steps[..] else {
        panic!("{steps:?}");
    };
    assert_eq!(failure.code, INSTALL_FAILED);
    assert!(failure.message.contains("/Volumes/Games"));
}

#[test]
fn a_failed_install_ends_with_the_engine_s_reason_once_it_is_told() {
    let follow = Install::new(VRCHAT);
    let failed = transferring(transfer(TransferState::Failed, 0, ""));
    assert!(
        follow.observe(&failed, Instant::now()).is_empty(),
        "its report follows"
    );
    let reported = InstallationsState {
        report: Some(InstallationReport::Storefront {
            key: "backend.steam.client.gameNotOwned".into(),
        }),
        ..failed
    };
    assert_eq!(
        told(&follow.observe(&reported, Instant::now())),
        ["failed not_owned"]
    );
    let voile = transferring(transfer(
        TransferState::Failed,
        0,
        "backend.voile.downloadFailed",
    ));
    assert_eq!(
        told(&follow.observe(&voile, Instant::now())),
        ["failed voile_failed"]
    );
    let cancelled = transferring(transfer(TransferState::Cancelled, 0, ""));
    assert_eq!(
        told(&follow.observe(&cancelled, Instant::now())),
        ["failed cancelled"]
    );
}

/// The Game sessions with none active.
fn no_session() -> GameSessionsState {
    GameSessionsState {
        revision: 1,
        active: None,
        last: None,
        recovering: false,
        recovery_failed: false,
        revoked: Vec::new(),
        rosetta: None,
    }
}

/// VRChat's Game session `id` in `phase`, for Gordon.
fn session(id: &str, phase: GamePhase, observed: bool) -> GameSessionState {
    GameSessionState {
        id: id.into(),
        game: GameIdentity::Steam {
            app_id: VRCHAT,
            account_id: GORDON,
        },
        phase,
        reconnecting: false,
        cloud: CloudStatus {
            phase: CloudPhase::Idle,
            decision_id: 0,
            error: None,
            allowed_choices: Vec::new(),
        },
        host: None,
        observed,
        runtime_install: None,
        voile_update: None,
        started_at: None,
        invite_request: None,
        lobby_id: None,
        overlay_request: None,
        received_invites: Vec::new(),
        micro_txn: None,
        played: None,
    }
}

fn active(session: GameSessionState) -> GameSessionsState {
    GameSessionsState {
        active: Some(session),
        ..no_session()
    }
}

/// The installations while VRChat's launch holds the game.
fn launching() -> InstallationsState {
    InstallationsState {
        installations: vec![vrchat()],
        launching: true,
        launch_game: Some("438100".into()),
        ..idle()
    }
}

/// The installations once VRChat runs: its launch still holds the game.
fn held() -> InstallationsState {
    InstallationsState {
        launching: false,
        ..launching()
    }
}

fn played(exit_code: Option<i32>) -> GamePlayed {
    GamePlayed {
        seconds: 3_600,
        frames: 100,
        frames_measured: true,
        median_fps: Some(72.0),
        low_1pct_fps: Some(40.0),
        hitches: 0,
        exit_code,
        exit_signal: None,
        ended_by_obelisk: false,
        engine: Some("linux".into()),
        graphics: Some("dxmt".into()),
        voile: Some("v26.3.0-18".into()),
    }
}

fn lines(steps: &[PlayStep]) -> Vec<String> {
    steps
        .iter()
        .map(|step| match step {
            PlayStep::Tell(event) => event.line(),
            PlayStep::TakeVoile { session } => format!("take voile {session}"),
            PlayStep::DeclineRosetta {
                session,
                decision_id,
            } => format!("decline rosetta {session} {decision_id}"),
            PlayStep::CancelCloud { decision_id } => format!("cancel cloud {decision_id}"),
            PlayStep::End(Ok(())) => "end".into(),
            PlayStep::End(Err(failure)) => format!("failed {}", failure.code),
        })
        .collect()
}

fn pid(id: &str) -> Option<u32> {
    (id == "supervised").then_some(4242)
}

#[test]
fn a_launch_tells_starting_then_running_with_the_game_s_process_then_its_exit() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    let admitted = active(session("admitted", GamePhase::Preparing, false));
    assert!(follow.observe(&launching(), &admitted, pid, now).is_empty());

    let supervised = active(session("supervised", GamePhase::Preparing, true));
    assert_eq!(
        lines(&follow.observe(&launching(), &supervised, pid, now)),
        [r#"{"event":"starting","app_id":438100}"#]
    );
    let running = active(session("supervised", GamePhase::Running, true));
    assert_eq!(
        lines(&follow.observe(&held(), &running, pid, now)),
        [r#"{"event":"running","app_id":438100,"pid":4242,"session_id":"supervised"}"#]
    );
    assert!(
        follow.observe(&held(), &running, pid, now).is_empty(),
        "each is told once"
    );
    let finalizing = active(session("supervised", GamePhase::Finalizing, true));
    assert!(follow.observe(&held(), &finalizing, pid, now).is_empty());

    let finished = GameSessionsState {
        last: Some(GameSessionState {
            played: Some(played(Some(0))),
            started_at: Some(1_790_000_000),
            ..session("supervised", GamePhase::Finished, true)
        }),
        ..no_session()
    };
    let released = InstallationsState {
        installations: vec![vrchat()],
        ..idle()
    };
    assert_eq!(
        lines(&follow.observe(&released, &finished, pid, now)),
        [
            r#"{"event":"exited","app_id":438100,"session_id":"supervised","frames":100,"exit_code":0,"exit_signal":null,"seconds":3600,"ended_by_request":false}"#,
            "end"
        ]
    );
}

#[test]
fn a_game_that_ran_and_exited_between_two_looks_still_tells_its_whole_life() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let finished = GameSessionsState {
        last: Some(GameSessionState {
            played: Some(played(Some(3))),
            ..session("supervised", GamePhase::Finished, true)
        }),
        ..no_session()
    };
    assert_eq!(
        lines(&follow.observe(&idle(), &finished, pid, Instant::now())),
        [
            r#"{"event":"starting","app_id":438100}"#,
            r#"{"event":"running","app_id":438100,"pid":null,"session_id":"supervised"}"#,
            r#"{"event":"exited","app_id":438100,"session_id":"supervised","frames":100,"exit_code":3,"exit_signal":null,"seconds":3600,"ended_by_request":false}"#,
            "end"
        ]
    );
}

#[test]
fn the_exit_told_is_the_running_session_s_own_never_another_s() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    let running = active(session("ours", GamePhase::Running, true));
    assert_eq!(follow.observe(&held(), &running, pid, now).len(), 2, "starting, running");
    // Ours ended, and another launch of the game in this library became the
    // last session before this look: its frames and exit are not ours.
    let other = GameSessionsState {
        last: Some(GameSessionState {
            played: Some(played(Some(0))),
            started_at: Some(1_790_000_000),
            ..session("someone-else-s", GamePhase::Finished, true)
        }),
        ..no_session()
    };
    let released = InstallationsState {
        installations: vec![vrchat()],
        ..idle()
    };
    assert_eq!(
        lines(&follow.observe(&released, &other, pid, now)),
        [
            r#"{"event":"exited","app_id":438100,"session_id":"ours","frames":null,"exit_code":null,"exit_signal":null,"seconds":null,"ended_by_request":false}"#,
            "end"
        ]
    );
}

#[test]
fn frames_the_game_s_log_could_not_tell_are_null_not_zero() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let unread = GameSessionsState {
        last: Some(GameSessionState {
            played: Some(GamePlayed {
                frames: 0,
                frames_measured: false,
                ..played(Some(0))
            }),
            ..session("supervised", GamePhase::Finished, true)
        }),
        ..no_session()
    };
    let told = lines(&follow.observe(&idle(), &unread, pid, Instant::now()));
    assert!(told.iter().any(|l| l.contains(r#""event":"exited""#) && l.contains(r#""frames":null"#)), "{told:?}");
}

#[test]
fn a_launch_takes_the_newer_voile_release_once_and_tells_its_runtimes_downloads() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    let offered = active(GameSessionState {
        voile_update: Some(VoileUpdateOffer {
            version: "v26.3.0-19.beta1".into(),
            required: true,
            releases: Vec::new(),
        }),
        ..session("admitted", GamePhase::Preparing, false)
    });
    assert_eq!(
        lines(&follow.observe(&launching(), &offered, pid, now)),
        [
            r#"{"event":"voile_update","app_id":438100,"version":"v26.3.0-19.beta1","required":true,"answer":"update"}"#,
            "take voile admitted"
        ]
    );
    assert!(follow.observe(&launching(), &offered, pid, now).is_empty());

    let downloading = |fraction| {
        active(GameSessionState {
            runtime_install: Some(RuntimeInstallState {
                component: RuntimeComponent::Voile,
                fraction,
            }),
            ..session("admitted", GamePhase::Preparing, false)
        })
    };
    assert_eq!(
        lines(&follow.observe(&launching(), &downloading(0.25), pid, now)),
        [
            r#"{"event":"progress","app_id":438100,"phase":"voile","bytes_done":null,"bytes_total":null,"fraction":0.25}"#
        ]
    );
    let soon = now + Duration::from_millis(50);
    assert!(
        follow
            .observe(&launching(), &downloading(0.3), pid, soon)
            .is_empty()
    );
}

#[test]
fn a_launch_that_repairs_or_updates_first_tells_that_transfer_s_progress() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let updating = InstallationsState {
        verifying_before_launch: true,
        transfer_game: Some("438100".into()),
        transfer: Some(transfer(TransferState::Downloading, 500, "")),
        downloading: true,
        ..launching()
    };
    let admitted = active(session("admitted", GamePhase::Preparing, false));
    assert_eq!(
        lines(&follow.observe(&updating, &admitted, pid, Instant::now())),
        [
            r#"{"event":"progress","app_id":438100,"phase":"downloading","bytes_done":500,"bytes_total":1000,"fraction":0.5}"#
        ]
    );
}

#[test]
fn a_launch_that_needs_rosetta_declines_its_install_and_fails_as_rosetta_required() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    let waiting = GameSessionsState {
        rosetta: Some(RosettaPromptState {
            session_id: "supervised".into(),
            account: GameAccount::Steam { steam_id: GORDON },
            decision_id: 7,
        }),
        ..active(session("supervised", GamePhase::Preparing, true))
    };
    assert_eq!(
        lines(&follow.observe(&launching(), &waiting, pid, now)),
        [
            r#"{"event":"starting","app_id":438100}"#,
            "decline rosetta supervised 7"
        ]
    );
    assert!(follow.observe(&launching(), &waiting, pid, now).is_empty());
    assert_eq!(
        lines(&follow.observe(&idle(), &no_session(), pid, now)),
        ["failed rosetta_required"]
    );
}

/// The installations while VRChat's Cloud sync asks `kind` for `sync`.
fn asking(sync: CloudSyncKind, kind: CloudQuestionKind) -> InstallationsState {
    InstallationsState {
        cloud_sync: Some(CloudSync {
            game: "438100".into(),
            kind: sync,
            decision_id: 3,
            working: false,
            reconnecting: false,
            cancellable: false,
            question: Some(CloudQuestion {
                decision_id: 3,
                kind,
                error: Some("backend.steam.cloud.conflict".into()),
                choices: vec![CloudChoice::Local, CloudChoice::Cloud, CloudChoice::Cancel],
            }),
        }),
        ..launching()
    }
}
use obelisk_bridge::CloudChoice;

#[test]
fn a_cloud_question_before_the_game_starts_is_cancelled_and_fails_the_launch() {
    for (kind, code) in [
        (CloudQuestionKind::Conflict, "cloud_conflict"),
        (CloudQuestionKind::Failed, "cloud_sync_failed"),
    ] {
        let mut follow = Play::new(VRCHAT, GORDON);
        let now = Instant::now();
        let supervised = active(session("supervised", GamePhase::Preparing, true));
        let question = asking(CloudSyncKind::Launch, kind);
        assert_eq!(
            lines(&follow.observe(&question, &supervised, pid, now)),
            [r#"{"event":"starting","app_id":438100}"#, "cancel cloud 3"]
        );
        assert!(follow.observe(&question, &supervised, pid, now).is_empty());
        assert_eq!(
            lines(&follow.observe(&idle(), &no_session(), pid, now)),
            [format!("failed {code}")],
            "never a guess at which saves to keep"
        );
    }
}

#[test]
fn a_cloud_question_after_the_game_exited_is_left_pending_and_the_exit_is_still_told() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    let running = active(session("supervised", GamePhase::Running, true));
    follow.observe(&held(), &running, pid, now);
    let question = InstallationsState {
        launching: false,
        ..asking(CloudSyncKind::Exit, CloudQuestionKind::Conflict)
    };
    let finalizing = active(session("supervised", GamePhase::Finalizing, true));
    assert_eq!(
        lines(&follow.observe(&question, &finalizing, pid, now)),
        [
            r#"{"event":"cloud_pending","app_id":438100,"kind":"conflict"}"#,
            "cancel cloud 3"
        ]
    );
    let finished = GameSessionsState {
        last: Some(GameSessionState {
            played: Some(played(Some(0))),
            ..session("supervised", GamePhase::Finished, true)
        }),
        ..no_session()
    };
    let ended = lines(&follow.observe(&idle(), &finished, pid, now));
    assert_eq!(ended.len(), 2);
    assert!(ended[0].starts_with(r#"{"event":"exited""#));
    assert_eq!(ended[1], "end");
}

#[test]
fn a_launch_that_fails_before_its_game_tells_the_engine_s_reason_and_one_cancelled_says_so() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let failed = InstallationsState {
        report: Some(InstallationReport::Storefront {
            key: "backend.runtime.wineIncomplete".into(),
        }),
        ..idle()
    };
    let steps = follow.observe(&failed, &no_session(), pid, Instant::now());
    let [PlayStep::End(Err(failure))] = &steps[..] else {
        panic!("{steps:?}");
    };
    assert_eq!(failure.code, LAUNCH_FAILED);
    assert_eq!(
        failure.key.as_deref(),
        Some("backend.runtime.wineIncomplete")
    );

    let mut cancelled = Play::new(VRCHAT, GORDON);
    assert_eq!(
        lines(&cancelled.observe(&idle(), &no_session(), pid, Instant::now())),
        ["failed cancelled"]
    );
}

#[test]
fn a_session_that_fails_after_its_game_ran_tells_the_exit_then_the_failure() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let now = Instant::now();
    follow.observe(
        &held(),
        &active(session("supervised", GamePhase::Running, true)),
        pid,
        now,
    );
    let failed = InstallationsState {
        report: Some(InstallationReport::Storefront {
            key: "backend.steam.runtime.storageFailed".into(),
        }),
        ..idle()
    };
    let last = GameSessionsState {
        last: Some(session("supervised", GamePhase::Failed, true)),
        ..no_session()
    };
    assert_eq!(
        lines(&follow.observe(&failed, &last, pid, now)),
        [
            r#"{"event":"exited","app_id":438100,"session_id":"supervised","frames":null,"exit_code":null,"exit_signal":null,"seconds":null,"ended_by_request":false}"#,
            "failed session_failed"
        ]
    );
}

#[test]
fn another_account_s_or_another_game_s_session_is_not_this_launch() {
    let mut follow = Play::new(VRCHAT, GORDON);
    let theirs = active(GameSessionState {
        game: GameIdentity::Steam {
            app_id: VRCHAT,
            account_id: GORDON + 1,
        },
        ..session("supervised", GamePhase::Running, true)
    });
    assert!(
        follow
            .observe(&launching(), &theirs, pid, Instant::now())
            .is_empty()
    );
    let other = active(GameSessionState {
        game: GameIdentity::Steam {
            app_id: 620,
            account_id: GORDON,
        },
        ..session("supervised", GamePhase::Running, true)
    });
    assert!(
        follow
            .observe(&launching(), &other, pid, Instant::now())
            .is_empty()
    );
}

#[test]
fn builds_and_phases_are_named_as_the_manual_names_them() {
    assert_eq!(platform(GamePlatform::Windows), "windows");
    assert_eq!(platform(GamePlatform::Mac), "mac");
    let phases = [
        GamePhase::Preparing,
        GamePhase::Running,
        GamePhase::Finalizing,
        GamePhase::Finished,
        GamePhase::Failed,
    ];
    assert_eq!(
        phases.map(phase),
        ["preparing", "running", "finalizing", "finished", "failed"]
    );
    assert_eq!(
        windows(&[vrchat()], "438100").map(|found| &found.id[..]),
        Some("438100-windows")
    );
    let mac = Installation {
        platform: GamePlatform::Mac,
        ..vrchat()
    };
    assert_eq!(windows(&[mac], "438100"), None);
}
