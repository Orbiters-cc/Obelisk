use super::*;

fn line(process: &str, pid: u32, first: u64, intervals: &[f32]) -> String {
    let frames: Vec<String> = intervals.iter().map(|i| format!("{i:.2},0.02")).collect();
    format!(
        "2026-10-14 09:04:41.290 {process}[{pid}:38256263] metal-HUD: {first},3.08,277.88,{}\n",
        frames.join(",")
    )
}

fn parsed(log: &str) -> Frames {
    parse(log.as_bytes()).unwrap()
}

#[test]
fn a_real_capture_reads_as_60_fps() {
    // Six seconds of a Metal test app presenting at 60 fps on a 120 Hz screen.
    let frames = parsed(include_str!("probe-60fps.log"));
    assert_eq!(frames.count, 363);
    assert_eq!(frames.median_us, Some(16_670));
    assert_eq!(frames.median_fps(), Some(60.0));
    assert_eq!(frames.low_1pct_fps(), Some(40.0));
    assert_eq!(frames.hitches, 0);
}

#[test]
fn the_process_with_the_most_frames_is_the_game() {
    let log = [
        line("Launcher.exe", 11, 1, &[16.67; 30]),
        "wine: some diagnostic\n".to_owned(),
        line("Game.exe", 12, 1, &[8.33; 120]),
        line("Game.exe", 12, 121, &[8.33; 120]),
    ]
    .concat();
    let frames = parsed(&log);
    assert_eq!(frames.count, 240);
    assert_eq!(frames.median_fps(), Some(120.0));
}

#[test]
fn the_slowest_percent_and_hitches_come_from_the_long_intervals() {
    let mut intervals = vec![10.0; 98];
    intervals.extend([50.0, 250.0]);
    let frames = parsed(&line("Game", 7, 1, &intervals));
    assert_eq!(frames.median_fps(), Some(100.0));
    assert_eq!(frames.low_1pct_fps(), Some(20.0));
    assert_eq!(frames.hitches, 1);
}

#[test]
fn an_uncapped_menu_reports_at_most_1000_fps() {
    let frames = parsed(&line("Game", 7, 1, &[0.25; 10]));
    assert_eq!(frames.median_fps(), Some(MAX_FPS));
    assert_eq!(frames.low_1pct_fps(), Some(MAX_FPS));
}

#[test]
fn a_log_without_hud_lines_reports_no_frames() {
    assert_eq!(Frames::default().median_fps(), None);
    let none = Frames {
        measured: true,
        ..Frames::default()
    };
    assert_eq!(parsed("wine: starting\nerr:module:import_dll\n"), none);
    assert_eq!(parsed(""), none);
}

#[test]
fn a_log_that_cannot_be_read_reports_an_unknown_count_not_zero() {
    let frames = read(Path::new("/nonexistent/game.log"));
    assert!(!frames.measured);
    assert_eq!(frames.count, 0);
    assert!(!Frames::default().measured, "no log at all is unknown too");
    // A record written before the field was read the same way: measured.
    let old: Frames = serde_json::from_str(
        r#"{"count":0,"median_us":null,"slowest_percent_us":null,"hitches":0}"#,
    )
    .unwrap();
    assert!(old.measured);
}

#[test]
fn the_counter_counts_as_the_whole_log_does_whatever_the_reads_cut() {
    let probe = include_str!("probe-60fps.log");
    let whole = parsed(probe).count;
    assert!(whole > 0);
    for size in [1, 7, 64, 4096] {
        let mut counter = Counter::default();
        for chunk in probe.as_bytes().chunks(size) {
            counter.feed(chunk);
        }
        // A last line without its newline is counted once it ends.
        counter.feed(b"\n");
        assert_eq!(counter.count(), whole, "reads of {size} bytes");
    }
    assert_eq!(Counter::default().count(), 0);
}

#[test]
fn malformed_values_and_invalid_utf8_are_skipped() {
    let mut log = line("Game", 7, 1, &[16.67; 4]).into_bytes();
    log.extend(b"\xff\xfe Game[7:1] metal-HUD: 5,1,1,abc,0.02,-3,0.02,16.67\n");
    let frames = parse(log.as_slice()).unwrap();
    assert_eq!(frames.count, 5);
}

#[test]
fn a_line_without_a_process_counts_as_process_zero() {
    assert_eq!(pid(b"metal"), 0);
    assert_eq!(pid(b"2026-10-14 09:04:41.290 Game[38516:38256263] "), 38516);
    assert_eq!(pid(b"Df probe[38314:2479811] (libMTLHud.dylib) "), 38314);
}

#[test]
fn the_hud_shows_over_the_game_only_when_asked() {
    let opacity = |shown| {
        environment(shown)
            .into_iter()
            .find(|(name, _)| *name == "MTL_HUD_OPACITY")
            .map(|(_, value)| value)
    };
    assert_eq!(opacity(false), Some("0"));
    assert_eq!(opacity(true), Some("1"));
    assert!(environment(true).contains(&("MTL_HUD_LOG_ENABLED", "1")));
}

#[test]
fn a_report_drops_every_line_the_frames_come_from() {
    let probe = include_str!("probe-60fps.log");
    assert!(parsed(probe).count > 0);
    assert_eq!(parsed(&without_frame_lines(probe)).count, 0);
    assert_eq!(
        without_frame_lines("a\n\nmetal-HUD:x\nb\r\nmetal-HUD: 1\n"),
        "a\n\nb\r\n"
    );
}
