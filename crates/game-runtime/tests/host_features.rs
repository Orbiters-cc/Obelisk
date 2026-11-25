//! What a process asks the engine's host programs for reaches the settings of
//! the games it launches. A test process of its own: it is asked once for all.
use game_runtime::{
    launch_settings::{self, HostFeatures},
    profile::Game,
};

#[test]
fn a_process_that_asks_the_engine_s_host_programs_has_it_where_the_game_s_engine_does() {
    let library = tempfile::tempdir().unwrap();
    let install = library.path().join("Games/Steam/steamapps/common/VRChat");
    std::fs::create_dir_all(&install).unwrap();
    let host = || launch_settings::of(library.path(), Game::Steam { app_id: 438100 }, &install, None).settings.host;
    assert_eq!(host(), HostFeatures::default(), "nothing by itself");
    let both = HostFeatures { free_pointer: true, shared_textures: true };
    assert!(launch_settings::ask_host_features(both));
    assert_eq!(host(), both, "VRChat's profile: the Linux engine with DXMT");
    // Kept nowhere, and the player's choice of another engine goes before it.
    assert!(!library.path().join("LaunchSettings").exists());
    std::fs::create_dir_all(library.path().join("LaunchSettings")).unwrap();
    std::fs::write(library.path().join("LaunchSettings/VRChat.json"), br#"{"engine":"Rosetta"}"#).unwrap();
    assert_eq!(host(), HostFeatures::default());
    assert!(!launch_settings::ask_host_features(HostFeatures::default()), "asked once");
}
