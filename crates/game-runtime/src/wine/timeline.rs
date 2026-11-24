//! The host timeline's variables (`docs/CLI.md`, "The timeline generation"): where the window server and the render server of
//! the timeline generation record, how much, and what. From this program's own environment, checked, and only when this
//! process was asked for that generation: the launcher (`launch.sh`) gives them to those two alone. Feature
//! `developer-timeline` only. The window server's switches (`OBELISK_WSD_UPSCALE`, `OBELISK_WSD_PRESENT`) and the render server's
//! (`OBELISK_DXMT_QUERY_RESCUE`, `OBELISK_DXMT_INIT_ORDER`) go the same way, with or without a recording.
use crate::voile::engine_patch::developer::{Generation, selected};
use std::path::{Path, PathBuf};

const FOLDER: &str = "OBELISK_TIMELINE";
/// The others, with what each may be.
const NUMBERS: [(&str, std::ops::RangeInclusive<u32>); 2] = [("OBELISK_TIMELINE_MB", 1..=8192), ("OBELISK_TIMELINE_PASSES", 0..=1000)];

/// The host programs' switches, with what each may be made of: the window server's two, then the render server's two.
const SWITCHES: [(&str, &[&str]); 4] = [
    ("OBELISK_WSD_UPSCALE", &["metalfx"]),
    ("OBELISK_WSD_PRESENT", &["arrival", "attime", "nosync", "two"]),
    ("OBELISK_DXMT_QUERY_RESCUE", &["once"]),
    ("OBELISK_DXMT_INIT_ORDER", &["1"]),
];

/// What the launcher's environment gets; `seen` are the folders the game sees.
pub(super) fn variables(seen: &[&Path]) -> Vec<(String, String)> {
    if selected() != Generation::Timeline {
        return Vec::new();
    }
    let asked = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let mut variables = match checked(asked(FOLDER).map(PathBuf::from), NUMBERS.map(|(name, _)| asked(name)), seen) {
        Ok(variables) => variables,
        Err(why) => {
            eprintln!("timeline: nothing is recorded: {why}");
            Vec::new()
        }
    };
    match switches(SWITCHES.map(|(name, _)| asked(name))) {
        Ok(switches) => variables.extend(switches),
        Err(why) => eprintln!("timeline: no host program has a switch: {why}"),
    }
    variables
}

fn switches(values: [Option<String>; 4]) -> Result<Vec<(String, String)>, String> {
    let mut variables = Vec::new();
    for ((name, known), value) in SWITCHES.into_iter().zip(values) {
        let Some(value) = value else { continue };
        if let Some(other) = value.split(',').find(|part| !known.contains(part)) {
            return Err(format!("{name}: {other:?} is none of {}", known.join(", ")));
        }
        variables.push((name.to_owned(), value));
    }
    Ok(variables)
}

fn checked(folder: Option<PathBuf>, numbers: [Option<String>; 2], seen: &[&Path]) -> Result<Vec<(String, String)>, String> {
    let Some(folder) = folder else { return Ok(Vec::new()) };
    if !folder.is_absolute() {
        return Err(format!("{} is not an absolute path", folder.display()));
    }
    let real = folder.canonicalize().map_err(|error| format!("{}: {error}", folder.display()))?;
    if !real.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    for place in seen {
        let place = place.canonicalize().unwrap_or_else(|_| place.to_path_buf());
        if real.starts_with(&place) {
            return Err(format!("{} is in {}, which the game sees", folder.display(), place.display()));
        }
    }
    let mut variables = vec![(FOLDER.to_owned(), real.to_string_lossy().into_owned())];
    for ((name, range), value) in NUMBERS.into_iter().zip(numbers) {
        let Some(value) = value else { continue };
        let number: u32 = value.parse().map_err(|_| format!("{name} is not a number"))?;
        if !range.contains(&number) {
            return Err(format!("{name} is not {} to {}", range.start(), range.end()));
        }
        variables.push((name.to_owned(), number.to_string()));
    }
    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_folder_the_game_cannot_see_and_numbers_in_range_are_passed_on() {
        let root = tempfile::tempdir().unwrap();
        let (trace, home) = (root.path().join("trace"), root.path().join("home"));
        std::fs::create_dir_all(&trace).unwrap();
        std::fs::create_dir_all(home.join("inside")).unwrap();
        let ask = |folder: &Path, mb: Option<&str>, passes: Option<&str>| {
            checked(Some(folder.to_owned()), [mb.map(str::to_owned), passes.map(str::to_owned)], &[&home])
        };
        let names = |variables: Vec<(String, String)>| variables.into_iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>();
        let told = names(ask(&trace, Some("256"), Some("8")).unwrap());
        assert!(told[0].starts_with("OBELISK_TIMELINE=/") && told[1..] == ["OBELISK_TIMELINE_MB=256", "OBELISK_TIMELINE_PASSES=8"], "{told:?}");
        assert_eq!(ask(&trace, None, None).unwrap().len(), 1);
        assert_eq!(checked(None, [Some("1".into()), None], &[]).unwrap(), []);
        assert!(ask(&home, None, None).is_err());
        assert!(ask(&home.join("inside"), None, None).is_err());
        assert!(ask(&root.path().join("missing"), None, None).is_err());
        assert!(ask(Path::new("trace"), None, None).is_err());
        std::os::unix::fs::symlink(home.join("inside"), root.path().join("link")).unwrap();
        assert!(ask(&root.path().join("link"), None, None).is_err(), "a link into what the game sees");
        for (mb, passes) in [(Some("0"), None), (Some("8193"), None), (Some("-1"), None), (None, Some("1001")), (None, Some("all")), (None, Some("1; rm"))] {
            assert!(ask(&trace, mb, passes).is_err(), "{mb:?} {passes:?}");
        }
    }

    #[test]
    fn the_host_programs_switches_are_the_known_ones() {
        let ask = |upscale: Option<&str>, present: Option<&str>, queries: Option<&str>, order: Option<&str>| switches([upscale, present, queries, order].map(|value| value.map(str::to_owned)));
        assert_eq!(ask(None, None, None, None).unwrap(), []);
        assert_eq!(ask(Some("metalfx"), Some("arrival,two"), Some("once"), Some("1")).unwrap().len(), 4);
        assert_eq!(ask(None, Some("arrival"), None, None).unwrap(), [("OBELISK_WSD_PRESENT".to_owned(), "arrival".to_owned())]);
        assert_eq!(ask(None, None, Some("once"), None).unwrap(), [("OBELISK_DXMT_QUERY_RESCUE".to_owned(), "once".to_owned())]);
        assert_eq!(ask(None, None, None, Some("1")).unwrap(), [("OBELISK_DXMT_INIT_ORDER".to_owned(), "1".to_owned())]);
        for wrong in [
            (Some("fsr"), None, None, None),
            (None, Some("arrival,"), None, None),
            (None, Some("arrival two"), None, None),
            (Some("metalfx;rm"), None, None, None),
            (None, None, Some("always"), None),
            (None, None, Some("1"), None),
            (None, None, None, Some("once")),
            (None, None, None, Some("0")),
        ] {
            assert!(ask(wrong.0, wrong.1, wrong.2, wrong.3).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn a_process_not_asked_for_the_generation_passes_nothing_on() {
        // (no test of this binary selects one)
        assert_eq!(variables(&[]), []);
    }
}
