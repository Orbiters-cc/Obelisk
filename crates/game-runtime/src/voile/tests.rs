use super::*;

/// Whether a test says that the disk of `place` exchanges nothing: a file
/// beside the runtimes' folder.
pub(super) fn no_exchange(place: &Path) -> bool {
    place.ancestors().skip(1).take(2).any(|folder| folder.join("no-exchange").exists())
}

/// Where `a_repair_killed_anywhere_leaves_a_whole_runtime` is killed, and what
/// takes a file where a test says so.
pub(super) fn step(name: &str) {
    if std::env::var("OBELISK_TEST_KILLED_AT").as_deref() == Ok(name) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", &std::process::id().to_string()])
            .status();
        std::thread::sleep(Duration::from_secs(10));
    }
    // (the staged runtime, or after the exchange the one in its place)
    if std::env::var("OBELISK_TEST_CUT_SHORT_AT").as_deref() == Ok(name) {
        let root = PathBuf::from(std::env::var_os("OBELISK_TEST_REPAIRED").unwrap()).join("Voile");
        let runtime = match name {
            "after-exchange" => root.join("v1.0.0-2"),
            _ => remains(&root).remove(0).join("voile-v1.0.0-2"),
        };
        fs::write(runtime.join("linux/wine/bin/wine"), "w").unwrap();
    }
}

/// A release archive laid out as voile's package.sh makes it, with the Linux
/// engine's runtime in its `linux` folder, and the release that pins it.
/// `manifest_version` is what its runtime.json claims.
fn release_archive(directory: &Path, version: &str, manifest_version: &str) -> (PathBuf, Release) {
    let package = directory.join("package");
    lay(
        &package,
        &format!("voile-{version}"),
        &format!(r#"{{"version": "{manifest_version}"}}"#),
    );
    packed(directory, version)
}

/// The archive of what is laid in `directory`'s `package`, and its release.
fn packed(directory: &Path, version: &str) -> (PathBuf, Release) {
    let package = directory.join("package");
    let archive = directory.join(format!("voile-{version}.tar.xz"));
    let status = std::process::Command::new("/usr/bin/tar")
        .arg("-cJf")
        .arg(&archive)
        .arg("-C")
        .arg(&package)
        .arg(format!("voile-{version}"))
        .status()
        .unwrap();
    assert!(status.success());
    let release = Release {
        version: version.into(),
        url: "https://example.invalid/voile.tar.xz".into(),
        size: fs::metadata(&archive).unwrap().len(),
        sha256: crate::sha(&archive).unwrap(),
    };
    (archive, release)
}

fn names(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A release's runtime in `root`, in `folder`, as its archive and an install
/// lay it, whose runtime.json holds `manifest`.
fn lay(root: &Path, folder: &str, manifest: &str) {
    let runtime = root.join(folder);
    fs::create_dir_all(runtime.join("bin")).unwrap();
    fs::write(runtime.join("bin/wine"), "wine").unwrap();
    fs::create_dir_all(runtime.join("linux/engine")).unwrap();
    fs::create_dir_all(runtime.join("linux/wine/bin")).unwrap();
    fs::write(runtime.join("linux/engine/fexdarwin-run"), "engine").unwrap();
    fs::write(runtime.join("linux/wine/bin/wine"), "wine").unwrap();
    fs::write(runtime.join(MANIFEST), manifest).unwrap();
}

/// The feed Obelisk read last, in `root`, offering `releases`.
fn feed(root: &Path, releases: &str) {
    feed_with_prereleases(root, releases, "");
}

/// The feed Obelisk read last, in `root`, offering `releases` and
/// `prereleases`.
fn feed_with_prereleases(root: &Path, releases: &str, prereleases: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join(feed::KEPT),
        format!(r#"{{"format": 1, "releases": [{releases}], "prereleases": [{prereleases}]}}"#),
    )
    .unwrap();
}

/// A release a feed offers, of `contract`.
fn offered(version: &str, contract: u32, withdrawn: Option<&str>) -> String {
    let withdrawn = withdrawn
        .map(|reason| format!(r#", "withdrawn": "{reason}""#))
        .unwrap_or_default();
    format!(
        r#"{{"version": "{version}", "contract": {contract}, "url": "https://example.invalid/voile-{version}.tar.xz", "size": 3, "sha256": "{}"{withdrawn}}}"#,
        "a".repeat(64)
    )
}

/// A release a feed offers, of the first contract, which brings `notes`.
fn noted(version: &str, notes: &str) -> String {
    offered(version, 1, None).replacen('{', &format!(r#"{{"notes": "{notes}", "#), 1)
}

#[test]
fn the_checked_in_release_names_its_archive_and_obelisk_s_contract() {
    let release = release();
    assert!(release.url.ends_with(&format!(
        "/releases/download/{0}/voile-{0}.tar.xz",
        release.version
    )));
    assert_eq!(release.sha256.len(), 64);
    assert!(is_version(&release.version));
    assert!(contract() >= 1);
}

#[test]
fn versions_order_by_their_numbers() {
    assert_eq!(numbers("v26.3.0-16"), Some(vec![26, 3, 0, 16, 1, 0]));
    assert!(numbers("v26.3.0-10") > numbers("v26.3.0-9"));
    assert!(numbers("v26.4.0-1") > numbers("v26.3.0-16"));
    assert_eq!(numbers("current"), None);
    assert_eq!(numbers("v26.3.0-x"), None);
    assert!(is_version("v26.3.0-16"));
    assert!(!is_version("v26.3"));
    assert!(!is_version("26.3.0-16"));
    assert!(!is_version("v26.3.0.1-16"));
    assert!(!is_version("v26.3.0-+16"));
    assert!(
        !is_version("v26.3.0-16-dev2"),
        "a local build is no release"
    );
}

#[test]
fn a_beta_comes_after_the_release_before_and_before_its_own() {
    assert_eq!(numbers("v26.3.0-16.beta2"), Some(vec![26, 3, 0, 16, 0, 2]));
    assert!(numbers("v26.3.0-15") < numbers("v26.3.0-16.beta1"));
    assert!(numbers("v26.3.0-16.beta1") < numbers("v26.3.0-16.beta2"));
    assert!(numbers("v26.3.0-16.beta9") < numbers("v26.3.0-16.beta10"));
    assert!(numbers("v26.3.0-16.beta10") < numbers("v26.3.0-16"));
    assert!(
        !is_version("v26.3.0-16.beta1"),
        "a profile names releases only"
    );
    assert!(is_beta("v26.3.0-16.beta1"));
    assert!(!is_beta("v26.3.0-16"));
    assert!(!is_beta("v26.3.0-16.beta"));
    assert!(!is_version("v26.3.0-16.beta"));
    assert!(!is_version("v26.3.0-16.betax"));
    assert!(!is_version("v26.3.0-16.beta1.beta2"));
}

#[test]
fn games_run_in_the_newest_release_of_obelisk_s_contract() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert_eq!(runtime(root), None);
    // A release before CONTRACT kept the first contract.
    lay(root, "v1.0.0-9", r#"{"version": "v1.0.0-9"}"#);
    lay(
        root,
        "v1.0.0-10",
        r#"{"version": "v1.0.0-10", "contract": 1}"#,
    );
    lay(
        root,
        "v1.0.0-11",
        r#"{"version": "v1.0.0-11", "contract": 2}"#,
    );
    lay(
        root,
        "v1.0.0-12",
        r#"{"version": "v1.0.0-1", "contract": 1}"#,
    );
    assert_eq!(runtime(root), Some(root.join("v1.0.0-10")));
    assert_eq!(linux_runtime(root), Some(root.join("v1.0.0-10/linux")));
    assert_eq!(installed_release(root), Some("v1.0.0-10".into()));
}

#[test]
fn a_withdrawn_release_runs_no_game_and_goes_with_the_others_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-1", r#"{"version": "v1.0.0-1"}"#);
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    lay(
        root,
        "v1.0.0-3",
        r#"{"version": "v1.0.0-3", "contract": 2}"#,
    );
    feed(
        root,
        &[
            offered("v1.0.0-2", 1, Some("breaks a game")),
            offered("v1.0.0-1", 1, None),
        ]
        .join(","),
    );
    fs::create_dir_all(root.join(".unfinished")).unwrap();
    assert_eq!(runtime(root), Some(root.join("v1.0.0-1")));

    remove_replaced(root);

    assert_eq!(names(root), [".unfinished", "feed.json", "v1.0.0-1"]);
}

#[test]
fn the_release_wanted_is_the_newest_the_feed_offers_for_obelisk_s_contract() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert_eq!(&Offer::read(root).wanted(), release(), "no feed read yet");
    feed(
        root,
        &[
            offered("v1.0.0-5", 1, Some("breaks a game")),
            offered("v1.0.0-4", 2, None),
            offered("v1.0.0-3", 1, None),
            offered("v1.0.0-2", 1, None),
        ]
        .join(","),
    );
    let wanted = Offer::read(root).wanted();
    assert_eq!(wanted.version, "v1.0.0-3");
    assert_eq!(wanted.url, "https://example.invalid/voile-v1.0.0-3.tar.xz");
}

/// Whether a launch in `root`, of a game that needs `minimum` or a newer
/// release, asks about a newer one, and must take it: `None` when it does
/// not ask.
fn asks(root: &Path, minimum: Option<&str>) -> Option<bool> {
    match standing(root, minimum) {
        Standing::Outdated(update) => Some(update.required),
        _ => None,
    }
}

#[test]
fn a_launch_waits_for_a_missing_release_and_asks_about_a_newer_one() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    feed(root, &offered("v1.0.0-3", 1, None));
    assert_eq!(standing(root, None), Standing::Missing);

    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    assert_eq!(asks(root, None), Some(false));
    assert_eq!(
        asks(root, Some("v1.0.0-3")),
        Some(true),
        "the game needs the release offered"
    );
    assert_eq!(
        asks(root, Some("v1.0.0-9")),
        Some(false),
        "no release offered is enough: the game runs in the newest it can"
    );
    assert_eq!(asks(root, Some("v1.0.0-2")), Some(false));

    lay(root, "v1.0.0-3", r#"{"version": "v1.0.0-3"}"#);
    assert_eq!(standing(root, Some("v1.0.0-3")), Standing::Current);

    fs::create_dir_all(root.join("current/bin")).unwrap();
    fs::write(root.join("current/bin/wine"), "wine").unwrap();
    feed(root, &offered("v1.0.0-4", 1, None));
    assert_eq!(standing(root, Some("v1.0.0-4")), Standing::Current);
}

#[test]
fn a_launch_offers_what_each_release_newer_than_the_one_in_place_brings() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    feed(
        root,
        &[
            offered("v1.0.0-6", 2, None),
            offered("v1.0.0-5", 1, Some("breaks a game")),
            noted("v1.0.0-4", "### Fixes\\n\\n- Fewer crashes"),
            offered("v1.0.0-3", 1, None),
            noted("v1.0.0-2", "Already in place"),
            noted("v1.0.0-1", "Older"),
        ]
        .join(","),
    );

    let notes = |version: &str, notes: &str| Notes {
        version: version.into(),
        notes: notes.into(),
    };
    assert_eq!(
        standing(root, None),
        Standing::Outdated(Update {
            version: "v1.0.0-4".into(),
            required: false,
            releases: vec![
                notes("v1.0.0-4", "### Fixes\n\n- Fewer crashes"),
                notes("v1.0.0-3", ""),
            ],
        })
    );
}

#[tokio::test]
async fn a_release_keeps_the_ones_before_it_until_they_are_removed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    fs::create_dir_all(root.join("v1.0.0-1/bin")).unwrap();
    fs::create_dir_all(root.join(".interrupted")).unwrap();

    let runtime = Install::new()
        .install(&root, &release, Some(&archive))
        .await
        .unwrap();

    assert_eq!(runtime, root.join("v1.0.0-2"));
    assert_eq!(self::runtime(&root), Some(runtime));
    assert_eq!(names(&root), ["v1.0.0-1", "v1.0.0-2"]);

    // Once no game may run from them, the ones before it go.
    fs::create_dir_all(root.join("current/bin")).unwrap();
    fs::create_dir_all(root.join(".unfinished")).unwrap();
    remove_replaced(&root);
    assert_eq!(names(&root), [".unfinished", "current", "v1.0.0-2"]);

    // The same release again is in place already.
    assert_eq!(
        Install::new()
            .install(&root, &release, Some(&root.join("no archive")))
            .await,
        Ok(root.join("v1.0.0-2"))
    );
}

#[tokio::test]
async fn a_damaged_archive_installs_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, mut release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    release.sha256 = "0".repeat(64);
    let failure = Install::new()
        .install(&root, &release, Some(&archive))
        .await;
    assert_eq!(failure, Err(Failure::Damaged));
    assert_eq!(names(&root), Vec::<String>::new());
}

#[tokio::test]
async fn an_archive_of_another_release_installs_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-1");
    let failure = Install::new()
        .install(&root, &release, Some(&archive))
        .await;
    assert_eq!(failure, Err(Failure::Damaged));
    assert_eq!(names(&root), Vec::<String>::new());
}

#[tokio::test]
async fn the_development_runtime_takes_precedence_over_any_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    fs::create_dir_all(root.join("current/bin")).unwrap();
    fs::write(root.join("current/bin/wine"), "wine").unwrap();
    let runtime = Install::new()
        .install(&root, &release, Some(&archive))
        .await
        .unwrap();
    assert_eq!(runtime, root.join("current"));
    assert_eq!(names(&root), ["current"]);
}

#[tokio::test]
async fn the_installed_release_is_the_runtime_games_run_in() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    assert_eq!(installed_release(&root), None);
    Install::new()
        .install(&root, &release, Some(&archive))
        .await
        .unwrap();
    assert_eq!(installed_release(&root), Some("v1.0.0-2".into()));
    fs::create_dir_all(root.join("current/bin")).unwrap();
    fs::write(root.join("current/bin/wine"), "wine").unwrap();
    fs::write(root.join("current").join(MANIFEST), r#"{"version": "dev"}"#).unwrap();
    assert_eq!(installed_release(&root), Some("dev".into()));
}

#[tokio::test]
async fn a_cancelled_install_stops_before_it_starts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    let install = Install::new();
    install.cancel();
    let failure = install.install(&root, &release, Some(&archive)).await;
    assert_eq!(failure, Err(Failure::Cancelled));
    assert!(!root.exists());
}

#[tokio::test]
async fn the_linux_runtime_comes_with_its_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    assert_eq!(linux_runtime(&root), None);
    Install::new()
        .install(&root, &release, Some(&archive))
        .await
        .unwrap();
    assert_eq!(linux_runtime(&root), Some(root.join("v1.0.0-2/linux")));
}

#[test]
fn a_linux_runtime_left_from_another_release_is_not_used() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    fs::create_dir_all(root.join("v1.0.0-2/linux/engine")).unwrap();
    fs::create_dir_all(root.join("v1.0.0-2/linux/wine/bin")).unwrap();
    fs::write(root.join("v1.0.0-2/linux/engine/fexdarwin-run"), "engine").unwrap();
    fs::write(root.join("v1.0.0-2/linux/wine/bin/wine"), "wine").unwrap();
    // No Windows runtime of this release around it.
    assert_eq!(linux_runtime(&root), None);
}

#[test]
fn the_development_linux_runtime_takes_precedence() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    fs::create_dir_all(root.join("current/linux/engine")).unwrap();
    fs::create_dir_all(root.join("current/linux/wine/bin")).unwrap();
    fs::write(root.join("current/linux/engine/fexdarwin-run"), "engine").unwrap();
    fs::write(root.join("current/linux/wine/bin/wine"), "wine").unwrap();
    assert_eq!(linux_runtime(&root), Some(root.join("current/linux")));
}

#[test]
fn a_player_without_beta_releases_leaves_a_pre_release_for_the_newest_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    feed_with_prereleases(
        root,
        &offered("v1.0.0-2", 1, None),
        &noted("v1.0.0-3", "A pre-release"),
    );
    assert_eq!(
        standing(root, None),
        Standing::Current,
        "nothing newer to take"
    );
    assert_eq!(
        asks(root, Some("v1.0.0-3")),
        None,
        "a game that needs a pre-release runs in the newest release"
    );

    // The pre-release a player took while they took beta releases.
    lay(root, "v1.0.0-3", r#"{"version": "v1.0.0-3"}"#);
    assert_eq!(runtime(root), Some(root.join("v1.0.0-2")));
    assert_eq!(linux_runtime(root), Some(root.join("v1.0.0-2/linux")));
    remove_replaced(root);
    assert_eq!(names(root), ["feed.json", "v1.0.0-2"]);

    // Once the release before it is gone, a launch waits for the release.
    fs::remove_dir_all(root.join("v1.0.0-2")).unwrap();
    lay(root, "v1.0.0-3", r#"{"version": "v1.0.0-3"}"#);
    assert_eq!(runtime(root), None);
    assert_eq!(standing(root, None), Standing::Missing);
    assert_eq!(Offer::read(root).wanted().version, "v1.0.0-2");
}

#[test]
fn a_player_who_takes_beta_releases_runs_the_newest_release_or_pre_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    feed_with_prereleases(
        root,
        &offered("v1.0.0-2", 1, None),
        &[
            offered("v1.0.0-5", 1, Some("breaks a game")),
            noted("v1.0.0-4", "Newest pre-release"),
            noted("v1.0.0-3", "Older pre-release"),
        ]
        .join(","),
    );
    take_prereleases(root, true).unwrap();
    take_prereleases(root, true).unwrap();

    let notes = |version: &str, notes: &str| Notes {
        version: version.into(),
        notes: notes.into(),
    };
    assert_eq!(
        standing(root, None),
        Standing::Outdated(Update {
            version: "v1.0.0-4".into(),
            required: false,
            releases: vec![
                notes("v1.0.0-4", "Newest pre-release"),
                notes("v1.0.0-3", "Older pre-release"),
            ],
        })
    );
    assert_eq!(asks(root, Some("v1.0.0-4")), Some(true));

    lay(root, "v1.0.0-4", r#"{"version": "v1.0.0-4"}"#);
    assert_eq!(runtime(root), Some(root.join("v1.0.0-4")));
    remove_replaced(root);
    assert_eq!(names(root), ["feed.json", "prereleases", "v1.0.0-4"]);

    // A release newer than every pre-release is the one wanted.
    feed_with_prereleases(
        root,
        &[offered("v1.0.0-6", 1, None), offered("v1.0.0-2", 1, None)].join(","),
        &offered("v1.0.0-4", 1, None),
    );
    assert_eq!(Offer::read(root).wanted().version, "v1.0.0-6");

    // Turned off, the pre-release in place is left for the newest release.
    take_prereleases(root, false).unwrap();
    take_prereleases(root, false).unwrap();
    assert_eq!(names(root), ["feed.json", "v1.0.0-4"]);
    assert_eq!(runtime(root), None);
    assert_eq!(standing(root, None), Standing::Missing);
}

#[test]
fn a_pre_release_turned_into_a_release_runs_for_every_player() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-3", r#"{"version": "v1.0.0-3"}"#);
    feed_with_prereleases(
        root,
        &[offered("v1.0.0-3", 1, None), offered("v1.0.0-2", 1, None)].join(","),
        "",
    );
    assert_eq!(runtime(root), Some(root.join("v1.0.0-3")));
    assert_eq!(standing(root, None), Standing::Current);
}

#[test]
fn a_player_who_takes_beta_releases_runs_the_newest_beta_until_its_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    lay(root, "v1.0.0-3.beta2", r#"{"version": "v1.0.0-3.beta2"}"#);
    feed_with_prereleases(
        root,
        &offered("v1.0.0-2", 1, None),
        &[
            offered("v1.0.0-3.beta10", 1, None),
            offered("v1.0.0-3.beta2", 1, None),
        ]
        .join(","),
    );
    assert_eq!(runtime(root), Some(root.join("v1.0.0-2")), "betas off");
    take_prereleases(root, true).unwrap();
    assert_eq!(runtime(root), Some(root.join("v1.0.0-3.beta2")));
    assert_eq!(Offer::read(root).wanted().version, "v1.0.0-3.beta10");
    assert!(
        matches!(standing(root, None), Standing::Outdated(update) if update.version == "v1.0.0-3.beta10")
    );

    // Its release comes after every beta of it.
    feed_with_prereleases(
        root,
        &[offered("v1.0.0-3", 1, None), offered("v1.0.0-2", 1, None)].join(","),
        &offered("v1.0.0-3.beta10", 1, None),
    );
    assert_eq!(Offer::read(root).wanted().version, "v1.0.0-3");
}

#[test]
fn a_beta_the_feed_no_longer_lists_is_still_left_without_beta_releases() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    lay(root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    lay(root, "v1.0.0-3.beta1", r#"{"version": "v1.0.0-3.beta1"}"#);
    feed(root, &offered("v1.0.0-2", 1, None));
    assert_eq!(runtime(root), Some(root.join("v1.0.0-2")));
    remove_replaced(root);
    assert_eq!(names(root), ["feed.json", "v1.0.0-2"]);

    lay(root, "v1.0.0-3.beta1", r#"{"version": "v1.0.0-3.beta1"}"#);
    take_prereleases(root, true).unwrap();
    assert_eq!(runtime(root), Some(root.join("v1.0.0-3.beta1")));

    // A beta turned into a release is one for every player.
    take_prereleases(root, false).unwrap();
    feed(
        root,
        &[
            offered("v1.0.0-3.beta1", 1, None),
            offered("v1.0.0-2", 1, None),
        ]
        .join(","),
    );
    assert_eq!(runtime(root), Some(root.join("v1.0.0-3.beta1")));
}

/// Fifth verification, findings 2 and 5: a repair lays the release again over
/// the one in place, whatever is missing or foreign in it, only once the new
/// one is whole, under the runtime's own lock.
#[tokio::test]
async fn a_repair_lays_the_release_again_in_one_step_and_never_under_a_game() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    let install = || async { Install::new().install(&root, &release, Some(&archive)).await };
    let again = || async { Install::new().put(&root, &release, Some(&archive), true).await };
    install().await.unwrap();
    let linux = root.join("v1.0.0-2/linux");
    let damage = || {
        fs::write(linux.join("engine/fexdarwin-run"), "damaged").unwrap();
        fs::write(linux.join(".obelisk-engine-patch.txn.json"), "{ half").unwrap();
    };
    damage();
    // An install finds the release in place; a repair lays it again.
    install().await.unwrap();
    assert_eq!(fs::read(linux.join("engine/fexdarwin-run")).unwrap(), b"damaged");
    assert_eq!(again().await, Ok(root.join("v1.0.0-2")));
    assert_eq!(fs::read(linux.join("engine/fexdarwin-run")).unwrap(), b"engine");
    assert!(!linux.join(".obelisk-engine-patch.txn.json").exists());
    assert_eq!(names(&root), ["v1.0.0-2"], "nothing is left beside it");

    // Never under a game, and nothing changes.
    damage();
    let pin = engine_patch::Pin::hold(&linux).unwrap();
    assert_eq!(again().await, Err(Failure::InUse));
    assert_eq!(fs::read(linux.join("engine/fexdarwin-run")).unwrap(), b"damaged");
    drop(pin);

    // A damaged download replaces nothing: what is in place, and what an
    // update left there, stay.
    let mut damaged = release.clone();
    damaged.sha256 = "0".repeat(64);
    let failed = Install::new().put(&root, &damaged, Some(&archive), true).await;
    assert_eq!(failed, Err(Failure::Damaged));
    assert!(linux.join(".obelisk-engine-patch.txn.json").exists());

    // Another process that installs is waited for.
    let held = engine_patch::installing(&root).unwrap().unwrap();
    let (install, waiting) = (Install::new(), again());
    tokio::select! {
        done = waiting => panic!("it did not wait: {done:?}"),
        () = tokio::time::sleep(Duration::from_millis(400)) => {}
    }
    drop(install);
    drop(held);
    assert_eq!(again().await, Ok(root.join("v1.0.0-2")));
}

/// Finding 2: an install lays a runtime's folder while no launch prepares in
/// it, and a release a game of any process holds is never removed.
#[tokio::test]
async fn a_replaced_release_a_game_holds_stays_until_it_is_let_go() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    lay(&root, "v1.0.0-1", r#"{"version": "v1.0.0-1"}"#);
    lay(&root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    let pin = engine_patch::Pin::hold(&root.join("v1.0.0-1/linux")).unwrap();
    remove_replaced(&root);
    assert_eq!(names(&root), ["v1.0.0-1", "v1.0.0-2"]);
    drop(pin);
    for _ in 0..200 {
        remove_replaced(&root);
        if names(&root) == ["v1.0.0-2"] {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{:?}", names(&root));
}

/// The release a repair lays again: the one games run in, else the newest
/// there is the folder of, whatever it lost.
#[test]
fn a_repair_names_the_release_in_place_whatever_it_lost() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    fs::create_dir_all(&root).unwrap();
    let named = || to_repair(&root, &Offer::read(&root));
    assert_eq!(named(), None);
    fs::create_dir_all(root.join("v1.0.0-1/linux")).unwrap();
    fs::create_dir_all(root.join("v1.0.0-3/linux")).unwrap();
    assert_eq!(named().as_deref(), Some("v1.0.0-3"));
    lay(&root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    assert_eq!(named().as_deref(), Some("v1.0.0-2"), "the one games run in");
}

#[test]
fn what_replaces_a_runtime_takes_its_place_in_one_step() {
    let directory = tempfile::tempdir().unwrap();
    let (new, place) = (directory.path().join("new"), directory.path().join("place"));
    for (folder, text) in [(&new, "new"), (&place, "old")] {
        fs::create_dir(folder).unwrap();
        fs::write(folder.join("file"), text).unwrap();
    }
    exchange(&new, &place).unwrap();
    assert_eq!(fs::read(place.join("file")).unwrap(), b"new");
    // The one it replaced is where the new one was, to go with its folder.
    assert_eq!(fs::read(new.join("file")).unwrap(), b"old");
    // A failure leaves the one in place.
    assert!(exchange(&directory.path().join("gone"), &place).is_err());
    assert_eq!(fs::read(place.join("file")).unwrap(), b"new");
}

/// Sixth verification, ruling 1: a disk that exchanges nothing is never
/// repaired over, and says so with the path, before any download.
#[tokio::test]
async fn a_disk_that_exchanges_nothing_is_not_repaired_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
    fs::write(root.join("v1.0.0-2/kept"), "").unwrap();
    fs::write(directory.path().join("no-exchange"), "").unwrap();
    let failure = Install::new().put(&root, &release, Some(&root.join("no archive")), true).await;
    assert_eq!(failure, Err(Failure::Exchange(root.clone())));
    let told: serde_json::Value = serde_json::from_str(&failure.unwrap_err().to_string()).unwrap();
    assert_eq!(told["key"], "backend.voile.noExchange");
    assert_eq!(told["args"][0], root.to_string_lossy().as_ref());
    assert!(root.join("v1.0.0-2/kept").exists());
    assert_eq!(names(&root), ["v1.0.0-2"]);
    // Seventh verification: nor is a release laid there anew, which no repair
    // could be made of: refused alike, before any download.
    let (archive, release) = release_archive(directory.path(), "v1.0.0-3", "v1.0.0-3");
    let laid = Install::new().install(&root, &release, Some(&root.join("no archive"))).await;
    assert_eq!(laid, Err(Failure::Exchange(root.clone())));
    assert_eq!(names(&root), ["v1.0.0-2"]);
    fs::remove_file(directory.path().join("no-exchange")).unwrap();
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
}

/// Sixth verification, finding 2 (the verifier's fixture): an extraction that
/// lost a file replaces nothing, the engine's own or any other of its archive.
#[tokio::test]
async fn an_extraction_that_lost_a_file_replaces_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
    let place = root.join("v1.0.0-2");
    let (staged, members) = Install::new().extract(&root, &release, Some(&archive)).await.unwrap();
    let runtime = staged.path().join("voile-v1.0.0-2");
    whole(&runtime, "v1.0.0-2", &members).unwrap();
    for lost in ["linux/engine/fexdarwin-run", "linux/wine/bin/wine", "bin/wine", MANIFEST] {
        let aside = directory.path().join("aside");
        fs::rename(runtime.join(lost), &aside).unwrap();
        assert_eq!(whole(&runtime, "v1.0.0-2", &members), Err(Failure::Damaged), "{lost}");
        fs::rename(&aside, runtime.join(lost)).unwrap();
    }
    drop(staged);
    assert!(place.join("linux/engine/fexdarwin-run").is_file());
    // The release our files are for carries an engine, whatever its archive
    // says: an archive without one is not that release.
    let version = engine_patch::release();
    let (archive, release) = release_archive(directory.path(), version, version);
    let damaged = Install::new().install(&root, &release, Some(&archive)).await;
    assert_eq!(damaged, Err(Failure::Damaged), "its engine files are not Voile's");
    assert_eq!(names(&root), ["v1.0.0-2"]);
}

/// Seventh verification, blocker 3 (the verifier's fixture): what was laid is
/// compared with the archive itself, member by member: a file cut short, of
/// another content, of another kind, that cannot be run any more, or a link
/// that leads elsewhere, is not the release.
#[tokio::test]
async fn a_staged_runtime_is_what_its_archive_holds_to_the_byte() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let version = "v1.0.0-2";
    let package = directory.path().join("package");
    lay(&package, "voile-v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    let laid = package.join("voile-v1.0.0-2");
    fs::create_dir_all(laid.join("share/é 日")).unwrap();
    fs::write(laid.join("share/é 日/a \\ b"), "odd").unwrap();
    fs::set_permissions(laid.join("linux/wine/bin/wine"), fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("wine", laid.join("linux/wine/bin/wine64")).unwrap();
    let (archive, release) = packed(directory.path(), version);
    let wine = "linux/wine/bin/wine";
    type Spoil = fn(&Path) -> std::io::Result<()>;
    let spoilt: [(&str, Spoil); 9] = [
        (wine, |path| fs::write(path, "wi")),
        (wine, |path| fs::write(path, "")),
        (wine, |path| fs::write(path, "wind")),
        (wine, |path| fs::set_permissions(path, fs::Permissions::from_mode(0o644))),
        (wine, |path| fs::remove_file(path)),
        (wine, |path| fs::rename(path, path.with_file_name("aside")).and_then(|()| std::os::unix::fs::symlink("aside", path))),
        ("linux/wine/bin/wine64", |path| fs::remove_file(path).and_then(|()| std::os::unix::fs::symlink("bash", path))),
        ("share/é 日/a \\ b", |path| fs::write(path, "ode")),
        ("share/é 日", |path| fs::remove_dir_all(path).and_then(|()| fs::write(path, ""))),
    ];
    for (member, spoil) in spoilt {
        let (staged, members) = Install::new().extract(&root, &release, Some(&archive)).await.unwrap();
        let runtime = staged.path().join("voile-v1.0.0-2");
        whole(&runtime, version, &members).unwrap();
        spoil(&runtime.join(member)).unwrap();
        assert_eq!(whole(&runtime, version, &members), Err(Failure::Damaged), "{member}");
    }
    // An archive that cannot be told member by member is no release.
    assert_eq!(names(&root), Vec::<String>::new());
    fs::hard_link(laid.join("bin/wine"), laid.join("bin/wine-again")).unwrap();
    let (archive, release) = packed(directory.path(), version);
    assert_eq!(Install::new().install(&root, &release, Some(&archive)).await, Err(Failure::Damaged));
    assert_eq!(names(&root), Vec::<String>::new());
}

/// Blocker 3: damaged between its check and its place, or in its place, a
/// runtime replaces nothing: what it was to replace is back.
#[test]
fn a_runtime_damaged_on_its_way_gives_the_place_back() {
    for (step, replaces) in [("after-validation", true), ("after-lock", true), ("after-exchange", true), ("after-validation", false)] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Voile");
        let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
        if replaces {
            lay(&root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
            fs::write(root.join("v1.0.0-2/old"), "").unwrap();
        }
        fs::write(
            directory.path().join("release"),
            format!("{}\n{}\n{}", archive.display(), release.size, release.sha256),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "voile::tests::a_repair_of_a_test_s"])
            .env("OBELISK_TEST_REPAIRED", directory.path())
            .env("OBELISK_TEST_CUT_SHORT_AT", step)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{step}");
        let place = root.join("v1.0.0-2");
        assert_eq!(place.join("old").exists(), replaces, "{step}");
        assert_eq!(place.exists(), replaces, "{step}");
        assert_eq!(remains(&root), Vec::<PathBuf>::new(), "{step}");
    }
}

/// Blocker 3: an install retires what one cut short saved of its own release,
/// once the new one is verified in its place, and never a whole runtime of
/// another release, which may be the only one there is.
#[tokio::test]
async fn a_saved_runtime_of_another_release_is_never_retired() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    lay(&root.join(".saved"), "voile-v1.0.0-1", r#"{"version": "v1.0.0-1"}"#);
    lay(&root.join(".own"), "voile-v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    fs::create_dir_all(root.join(".part/voile-v1.0.0-1/bin")).unwrap();
    fs::write(root.join(".download"), "").unwrap();
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
    assert_eq!(names(&root), [".saved", "v1.0.0-2"]);
    let (archive, release) = release_archive(directory.path(), "v1.0.0-1", "v1.0.0-1");
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
    assert_eq!(names(&root), ["v1.0.0-1", "v1.0.0-2"]);
}

/// Finding 2 (the verifier's matrix): killed at any step, a repair leaves a
/// whole runtime in place, the old one or the new one, and one that failed
/// after it deletes nothing saved until a whole one is in place.
#[test]
fn a_repair_killed_anywhere_leaves_a_whole_runtime() {
    for (step, expected) in [
        ("before-extract", "old"),
        ("after-untar", "old"),
        ("after-extract", "old"),
        ("after-validation", "old"),
        ("after-lock", "old"),
        ("after-exchange", "new"),
        ("after-verification", "new"),
        ("after-cleanup", "new"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Voile");
        let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
        lay(&root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
        fs::write(root.join("v1.0.0-2/old"), "").unwrap();
        fs::write(
            directory.path().join("release"),
            format!("{}\n{}\n{}", archive.display(), release.size, release.sha256),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "voile::tests::a_repair_of_a_test_s"])
            .env("OBELISK_TEST_REPAIRED", directory.path())
            .env("OBELISK_TEST_KILLED_AT", step)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), Some(9), "{step}");
        let place = root.join("v1.0.0-2");
        let whole = || {
            has_wine(&place) && has_engine(&place.join(LINUX)) && version(&place).as_deref() == Some("v1.0.0-2")
        };
        assert!(whole(), "{step}");
        assert_eq!(place.join("old").exists(), expected == "old", "{step}");
        let saved = || {
            remains(&root).iter().any(|staged| staged.join("voile-v1.0.0-2/old").exists())
        };
        if step == "after-exchange" {
            assert!(saved(), "the one replaced is kept until the new one is verified in place");
        }
        // The next one cannot be made: nothing saved goes.
        let kept = remains(&root).len();
        let executor = crate::executor().unwrap();
        let again = |archive: &Path| {
            executor.block_on(async { Install::new().put(&root, &release, Some(archive), true).await })
        };
        assert!(again(&directory.path().join("no archive")).is_err());
        assert!(whole(), "{step}");
        assert_eq!(remains(&root).len(), kept, "{step}");
        // Then it is, and nothing is left.
        again(&archive).unwrap();
        assert!(whole() && !place.join("old").exists());
        assert_eq!(names(&root), ["v1.0.0-2"], "{step}");
    }
}

/// The folders an install left in `root`.
fn remains(root: &Path) -> Vec<PathBuf> {
    names(root)
        .into_iter()
        .filter(|name| name.starts_with(UNFINISHED))
        .map(|name| root.join(name))
        .filter(|path| path.is_dir())
        .collect()
}

/// Not a test of its own: the repair the one above kills.
#[tokio::test]
async fn a_repair_of_a_test_s() {
    let Some(directory) = std::env::var_os("OBELISK_TEST_REPAIRED").map(PathBuf::from) else {
        return;
    };
    let told = fs::read_to_string(directory.join("release")).unwrap();
    let told: Vec<&str> = told.lines().collect();
    let release = Release {
        version: "v1.0.0-2".into(),
        url: "https://example.invalid/voile.tar.xz".into(),
        size: told[1].parse().unwrap(),
        sha256: told[2].into(),
    };
    let _ = Install::new()
        .put(&directory.join("Voile"), &release, Some(Path::new(told[0])), true)
        .await;
}

/// Ruling 2: a repair never replaces a runtime while anything of its
/// library's engine runs, whatever the locks say; a release that is not there
/// yet can be installed beside the one a game runs in.
#[tokio::test]
async fn a_repair_waits_for_whatever_runs_and_a_new_release_does_not() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let (archive, release) = release_archive(directory.path(), "v1.0.0-2", "v1.0.0-2");
    Install::new().install(&root, &release, Some(&archive)).await.unwrap();
    let running = live::tests::Running::at(&root.join("v1.0.0-2/linux/graphics/bin/virgl_test_server"));
    assert_eq!(Install::new().put(&root, &release, Some(&archive), true).await, Err(Failure::InUse));
    assert!(root.join("v1.0.0-2/linux/graphics/bin/virgl_test_server").exists());
    let (newer_archive, newer) = release_archive(directory.path(), "v1.0.0-3", "v1.0.0-3");
    Install::new().install(&root, &newer, Some(&newer_archive)).await.unwrap();
    remove_replaced(&root);
    assert_eq!(names(&root), ["v1.0.0-2", "v1.0.0-3"], "nor is the one it replaced removed");
    running.end();
    remove_replaced(&root);
    assert_eq!(names(&root), ["v1.0.0-3"]);
}

/// Ruling 6: the engine of the release in place is named whatever it lost, so
/// that whoever starts a game there says what.
#[test]
fn an_engine_that_lost_its_program_is_still_the_one_games_run_on() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    lay(&root, "v1.0.0-2", r#"{"version": "v1.0.0-2"}"#);
    let linux = root.join("v1.0.0-2/linux");
    required::tests::lay(&linux);
    let missing = |linux: &Path| lacking(linux, crate::wine::Engine::Linux, crate::wine::Graphics::Dxmt);
    assert_eq!(missing(&linux), Vec::<String>::new());
    fs::remove_file(linux.join("engine/fexdarwin-run")).unwrap();
    assert_eq!(linux_runtime(&root), Some(linux.clone()));
    assert_eq!(missing(&linux), ["engine/fexdarwin-run"]);
    // A release without an engine at all has none, but for the one whose
    // engine files are ours to patch.
    fs::remove_dir_all(&linux).unwrap();
    assert_eq!(linux_runtime(&root), None);
    let ours = engine_patch::release();
    take_prereleases(&root, true).unwrap();
    lay(&root, ours, &format!(r#"{{"version": "{ours}"}}"#));
    fs::remove_dir_all(root.join(ours).join(LINUX)).unwrap();
    assert_eq!(linux_runtime(&root), Some(root.join(ours).join(LINUX)));
}

/// With `OBELISK_VOILE_ARCHIVE` naming a real archive, `voile-<version>.tar.xz`:
/// laid, found whole, and laid again over itself with our files in it.
#[tokio::test]
#[ignore = "needs OBELISK_VOILE_ARCHIVE and 5 GB"]
async fn a_real_release_is_found_whole() {
    let archive = PathBuf::from(std::env::var_os("OBELISK_VOILE_ARCHIVE").expect("an archive"));
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    let release = Release {
        version: name.strip_prefix("voile-").unwrap().strip_suffix(".tar.xz").unwrap().to_owned(),
        url: "https://example.invalid/voile.tar.xz".into(),
        size: fs::metadata(&archive).unwrap().len(),
        sha256: crate::sha(&archive).unwrap(),
    };
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Voile");
    let put = |again| {
        let (root, release, archive) = (root.clone(), release.clone(), archive.clone());
        async move { Install::new().put(&root, &release, Some(&archive), again).await }
    };
    let place = put(false).await.unwrap();
    let linux = place.join(LINUX);
    if release.version == engine_patch::release() {
        assert_eq!(engine_patch::apply(&linux).unwrap().0, engine_patch::Keyboard::Mac);
    }
    fs::remove_file(linux.join("engine/fexdarwin-run")).unwrap();
    assert_eq!(put(true).await.unwrap(), place);
    assert!(has_engine(&linux) && engine_patch::pristine(&linux, &release.version));
    assert_eq!(names(&root), [&*release.version]);
}
