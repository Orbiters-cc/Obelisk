//! The command line, read without a library: a few commands, each with a few
//! options, and a game's own arguments passed on as they are after `--`.
use crate::failure::{Failure, USAGE};
use std::path::PathBuf;

/// What `--help` writes to standard error.
pub const HELP: &str = "\
obelisk-cli: Obelisk's engine without its window. Every command writes JSON Lines to standard output.

Usage: obelisk-cli [--library <dir>] [--helpers <dir>] <command>

Commands:
  status                              The signed-in account, the Voile release, the helpers
  steam login --qr                    Sign in by scanning a QR code with the Steam mobile app
  steam login --account <name> [--guard <kind>]
                                      Sign in with a password read from standard input
  steam logout                        Forget the signed-in account
  steam installed                     List the installed games
  steam install <app_id> [--language <locale>]
                                      Install or update the Windows build of a game
  steam play <app_id> [--steam-id <id>] [--render-percent <1..100>] [-- <args>...]
                                      Start an installed game and follow it until it ends;
                                      --steam-id starts it for that Steam account or not at all,
                                      --render-percent draws it at that share of the display's
                                      pixels per side, for this launch only (Linux engine)
  steam running                       List the live Game sessions
  steam quit <app_id> [--session <id>] [--force] [--wait <seconds>]
                                      Ask a running game to end
  engine repair                       Install the Voile release in place again, checked
  settings prereleases on|off         Take Voile's pre-releases too, or not

Options:
  --library <dir>   Where games, runtimes and sessions are kept
                    (default: ~/Library/Application Support/cc.orbiters.obelisk-cli)
  --helpers <dir>   The folder that holds obelisk-steam-host (default: SteamNative beside this program)
  --help, --version

docs/CLI.md describes every event and error code.
";

/// One invocation: where it works, and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub library: Option<PathBuf>,
    pub helpers: Option<PathBuf>,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Status,
    Login(Login),
    Logout,
    Installed,
    Install {
        app_id: u32,
        /// A locale such as `fr-FR`, whose Steam language the game installs.
        language: Option<String>,
    },
    Play {
        app_id: u32,
        /// The only Steam account the game may start for.
        steam_id: Option<u64>,
        /// What starts the game after its own arguments, as given.
        arguments: Vec<String>,
        /// The render percent of this launch alone, over the installation's own choice.
        render_percent: Option<u32>,
    },
    Running,
    Quit {
        app_id: u32,
        force: bool,
        /// How long to wait for the game to end, in seconds.
        wait: Option<u64>,
        /// Only the Game session with this ID is asked to end.
        session: Option<String>,
    },
    EngineRepair,
    Prereleases(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Login {
    Qr,
    Account {
        name: String,
        /// The confirmation to use among those Steam offers.
        guard: Option<Guard>,
    },
}

/// A confirmation of a sign-in by account name and password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    DeviceConfirmation,
    DeviceCode,
    EmailCode,
    EmailConfirmation,
}

impl Guard {
    pub const ALL: [Self; 4] = [
        Self::DeviceConfirmation,
        Self::DeviceCode,
        Self::EmailCode,
        Self::EmailConfirmation,
    ];

    /// How the command line and the `guard` event name it.
    pub fn name(self) -> &'static str {
        match self {
            Self::DeviceConfirmation => "device_confirmation",
            Self::DeviceCode => "device_code",
            Self::EmailCode => "email_code",
            Self::EmailConfirmation => "email_confirmation",
        }
    }

    fn named(name: &str) -> Option<Self> {
        let name = name.replace('-', "_");
        Self::ALL.into_iter().find(|guard| guard.name() == name)
    }
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::new(USAGE, message)
}

/// The words of a command line, in order.
struct Words {
    words: std::vec::IntoIter<String>,
}

impl Words {
    fn next(&mut self) -> Option<String> {
        self.words.next()
    }

    /// The value the option `name` takes.
    fn value(&mut self, name: &str) -> Result<String, Failure> {
        self.next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| usage(format!("{name} takes a value.")))
    }
}

/// A Steam app ID: a positive whole number.
fn app_id(word: Option<String>, command: &str) -> Result<u32, Failure> {
    let word = word.ok_or_else(|| usage(format!("{command} takes a Steam app ID.")))?;
    match word.parse::<u32>() {
        Ok(app_id) if app_id != 0 && word.bytes().all(|byte| byte.is_ascii_digit()) => Ok(app_id),
        _ => Err(usage(format!("{word} is not a Steam app ID."))),
    }
}

/// Reads the command line after the program's name.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Invocation, Failure> {
    let mut words = Words {
        words: arguments.into_iter().collect::<Vec<_>>().into_iter(),
    };
    let (mut library, mut helpers) = (None, None);
    // What is neither an option of every command nor a game's own argument.
    let mut rest = Vec::new();
    let mut game = Vec::new();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--" => {
                game.extend(words.words.by_ref());
                rest.push(word);
            }
            "--library" => library = Some(PathBuf::from(words.value("--library")?)),
            "--helpers" => helpers = Some(PathBuf::from(words.value("--helpers")?)),
            "--help" | "-h" => rest.insert(0, "--help".into()),
            "--version" | "-V" => rest.insert(0, "--version".into()),
            _ => rest.push(word),
        }
    }
    let command = command(
        Words {
            words: rest.into_iter(),
        },
        game,
    )?;
    Ok(Invocation {
        library,
        helpers,
        command,
    })
}

fn command(mut words: Words, game: Vec<String>) -> Result<Command, Failure> {
    let first = words
        .next()
        .ok_or_else(|| usage("A command is missing: see --help."))?;
    let command = match first.as_str() {
        "--help" => return Ok(Command::Help),
        "--version" => return Ok(Command::Version),
        "status" => Command::Status,
        "steam" => steam(&mut words)?,
        "engine" => match words.next().as_deref() {
            Some("repair") => Command::EngineRepair,
            _ => return Err(usage("engine takes repair.")),
        },
        "settings" => settings(&mut words)?,
        other => return Err(usage(format!("{other} is not a command: see --help."))),
    };
    match words.next().as_deref() {
        None => {}
        Some("--") if matches!(command, Command::Play { .. }) => {}
        Some("--") => return Err(usage("Only steam play takes arguments after --.")),
        Some(word) => return Err(usage(format!("{word} is not expected here: see --help."))),
    }
    Ok(match command {
        Command::Play { app_id, steam_id, render_percent, .. } => Command::Play {
            app_id,
            steam_id,
            arguments: game,
            render_percent,
        },
        command => command,
    })
}

fn steam(words: &mut Words) -> Result<Command, Failure> {
    let action = words
        .next()
        .ok_or_else(|| usage("steam takes a command: see --help."))?;
    Ok(match action.as_str() {
        "login" => Command::Login(login(words)?),
        "logout" => Command::Logout,
        "installed" => Command::Installed,
        "running" => Command::Running,
        "install" => {
            let app_id = app_id(words.next(), "steam install")?;
            let mut language = None;
            if words.words.as_slice().first().map(String::as_str) == Some("--language") {
                words.next();
                language = Some(words.value("--language")?);
            }
            Command::Install { app_id, language }
        }
        "play" => {
            let app_id = app_id(words.next(), "steam play")?;
            let (mut steam_id, mut render_percent) = (None, None);
            loop {
                match words.words.as_slice().first().map(String::as_str) {
                    Some("--steam-id") => {
                        words.next();
                        let value = words.value("--steam-id")?;
                        steam_id = match value.parse::<u64>() {
                            Ok(id) if id != 0 && value.bytes().all(|byte| byte.is_ascii_digit()) => Some(id),
                            _ => return Err(usage("--steam-id takes a Steam account's number.")),
                        };
                    }
                    Some("--render-percent") => {
                        words.next();
                        let value = words.value("--render-percent")?;
                        render_percent = match value.parse::<u32>() {
                            Ok(percent @ 1..=100) if value.bytes().all(|byte| byte.is_ascii_digit()) => Some(percent),
                            _ => return Err(usage(format!("--render-percent takes 1 to 100, not {value}."))),
                        };
                    }
                    _ => break,
                }
            }
            Command::Play { app_id, steam_id, arguments: Vec::new(), render_percent }
        }
        "quit" => {
            let app_id = app_id(words.next(), "steam quit")?;
            let (mut force, mut wait, mut session) = (false, None, None);
            loop {
                match words.words.as_slice().first().map(String::as_str) {
                    Some("--session") => {
                        words.next();
                        session = Some(words.value("--session")?);
                    }
                    Some("--force") => {
                        words.next();
                        force = true;
                    }
                    Some("--wait") => {
                        words.next();
                        let seconds = words.value("--wait")?;
                        wait = Some(seconds.parse::<u64>().map_err(|_| {
                            usage(format!("--wait takes whole seconds, not {seconds}."))
                        })?);
                    }
                    _ => break,
                }
            }
            Command::Quit {
                app_id,
                force,
                wait,
                session,
            }
        }
        other => {
            return Err(usage(format!(
                "steam {other} is not a command: see --help."
            )));
        }
    })
}

fn login(words: &mut Words) -> Result<Login, Failure> {
    let (mut qr, mut name, mut guard) = (false, None, None);
    loop {
        match words.words.as_slice().first().map(String::as_str) {
            Some("--qr") => {
                words.next();
                qr = true;
            }
            Some("--account") => {
                words.next();
                name = Some(words.value("--account")?);
            }
            Some("--guard") => {
                words.next();
                let kind = words.value("--guard")?;
                guard = Some(Guard::named(&kind).ok_or_else(|| {
                    usage(format!(
                        "--guard takes device-confirmation, device-code, email-code or email-confirmation, not {kind}."
                    ))
                })?);
            }
            _ => break,
        }
    }
    match (qr, name) {
        (true, None) if guard.is_none() => Ok(Login::Qr),
        (false, Some(name)) => Ok(Login::Account { name, guard }),
        _ => Err(usage(
            "steam login takes --qr, or --account <name> with an optional --guard <kind>.",
        )),
    }
}

fn settings(words: &mut Words) -> Result<Command, Failure> {
    match (words.next().as_deref(), words.next().as_deref()) {
        (Some("prereleases"), Some("on")) => Ok(Command::Prereleases(true)),
        (Some("prereleases"), Some("off")) => Ok(Command::Prereleases(false)),
        _ => Err(usage("settings takes prereleases on or prereleases off.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &[&str]) -> Result<Invocation, Failure> {
        parse(line.iter().map(|&word| word.to_owned()))
    }

    fn command_of(line: &[&str]) -> Command {
        read(line).unwrap().command
    }

    #[test]
    fn every_command_reads_as_the_manual_writes_it() {
        assert_eq!(command_of(&["status"]), Command::Status);
        assert_eq!(
            command_of(&["steam", "login", "--qr"]),
            Command::Login(Login::Qr)
        );
        assert_eq!(
            command_of(&["steam", "login", "--account", "gordon"]),
            Command::Login(Login::Account {
                name: "gordon".into(),
                guard: None
            })
        );
        assert_eq!(
            command_of(&[
                "steam",
                "login",
                "--guard",
                "email-code",
                "--account",
                "gordon"
            ]),
            Command::Login(Login::Account {
                name: "gordon".into(),
                guard: Some(Guard::EmailCode)
            })
        );
        assert_eq!(command_of(&["steam", "logout"]), Command::Logout);
        assert_eq!(command_of(&["steam", "installed"]), Command::Installed);
        assert_eq!(command_of(&["steam", "running"]), Command::Running);
        assert_eq!(
            command_of(&["steam", "install", "438100"]),
            Command::Install {
                app_id: 438100,
                language: None
            }
        );
        assert_eq!(
            command_of(&["steam", "install", "438100", "--language", "fr-FR"]),
            Command::Install {
                app_id: 438100,
                language: Some("fr-FR".into())
            }
        );
        assert_eq!(
            command_of(&["steam", "quit", "438100"]),
            Command::Quit {
                app_id: 438100,
                force: false,
                wait: None,
                session: None
            }
        );
        assert_eq!(
            command_of(&["steam", "quit", "438100", "--wait", "30", "--force"]),
            Command::Quit {
                app_id: 438100,
                force: true,
                wait: Some(30),
                session: None
            }
        );
        assert_eq!(
            command_of(&["steam", "quit", "438100", "--session", "f2931045-90d3-4302-a8a9-94021747c54f", "--wait", "30"]),
            Command::Quit {
                app_id: 438100,
                force: false,
                wait: Some(30),
                session: Some("f2931045-90d3-4302-a8a9-94021747c54f".into())
            }
        );
        assert_eq!(command_of(&["engine", "repair"]), Command::EngineRepair);
        assert_eq!(
            command_of(&["settings", "prereleases", "on"]),
            Command::Prereleases(true)
        );
        assert_eq!(
            command_of(&["settings", "prereleases", "off"]),
            Command::Prereleases(false)
        );
        assert_eq!(command_of(&["--help"]), Command::Help);
        assert_eq!(command_of(&["steam", "play", "-h"]), Command::Help);
        assert_eq!(command_of(&["--version"]), Command::Version);
    }

    #[test]
    fn a_game_s_arguments_pass_as_they_are_after_the_two_dashes() {
        assert_eq!(
            command_of(&["steam", "play", "438100"]),
            Command::Play {
                app_id: 438100,
                steam_id: None,
                arguments: Vec::new(),
                render_percent: None
            }
        );
        assert_eq!(
            command_of(&[
                "steam",
                "play",
                "438100",
                "--",
                "--no-vr",
                "--library",
                "two words",
                "--",
                "--help"
            ]),
            Command::Play {
                app_id: 438100,
                steam_id: None,
                arguments: vec![
                    "--no-vr".into(),
                    "--library".into(),
                    "two words".into(),
                    "--".into(),
                    "--help".into()
                ],
                render_percent: None
            }
        );
        assert_eq!(
            command_of(&["steam", "play", "438100", "--"]),
            Command::Play {
                app_id: 438100,
                steam_id: None,
                arguments: Vec::new(),
                render_percent: None
            }
        );
    }

    #[test]
    fn a_render_percent_is_for_one_launch_and_comes_before_the_game_s_arguments() {
        assert_eq!(
            command_of(&["steam", "play", "438100", "--render-percent", "58", "--", "--no-vr"]),
            Command::Play {
                app_id: 438100,
                steam_id: None,
                arguments: vec!["--no-vr".into()],
                render_percent: Some(58)
            }
        );
        // After the two dashes it is the game's.
        assert_eq!(
            command_of(&["steam", "play", "438100", "--", "--render-percent", "58"]),
            Command::Play {
                app_id: 438100,
                steam_id: None,
                arguments: vec!["--render-percent".into(), "58".into()],
                render_percent: None
            }
        );
    }

    #[test]
    fn the_account_a_game_starts_for_is_named_before_its_arguments() {
        for line in [
            &["steam", "play", "438100", "--steam-id", "76561197960265729", "--render-percent", "58", "--", "--no-vr"][..],
            &["steam", "play", "438100", "--render-percent", "58", "--steam-id", "76561197960265729", "--", "--no-vr"],
        ] {
            assert_eq!(
                command_of(line),
                Command::Play {
                    app_id: 438100,
                    steam_id: Some(76561197960265729),
                    arguments: vec!["--no-vr".into()],
                    render_percent: Some(58)
                }
            );
        }
    }

    #[test]
    fn the_library_and_the_helpers_are_named_anywhere_before_the_game_s_arguments() {
        let first = read(&["--library", "/tmp/library", "steam", "installed"]).unwrap();
        assert_eq!(first.library, Some(PathBuf::from("/tmp/library")));
        assert_eq!(first.helpers, None);
        assert_eq!(first.command, Command::Installed);
        let last = read(&[
            "steam",
            "play",
            "438100",
            "--helpers",
            "/opt/helpers",
            "--library",
            "relative",
        ])
        .unwrap();
        assert_eq!(last.library, Some(PathBuf::from("relative")));
        assert_eq!(last.helpers, Some(PathBuf::from("/opt/helpers")));
    }

    #[test]
    fn what_cannot_be_read_is_a_usage_failure() {
        for line in [
            &[][..],
            &["play"],
            &["steam"],
            &["steam", "play"],
            &["steam", "play", "vrchat"],
            &["steam", "play", "0"],
            &["steam", "play", "+438100"],
            &["steam", "play", "438100", "--no-vr"],
            &["steam", "play", "438100", "--steam-id"],
            &["steam", "play", "438100", "--steam-id", "0"],
            &["steam", "play", "438100", "--steam-id", "gordon"],
            &["steam", "play", "438100", "--steam-id", "+76561197960265729"],
            &["steam", "play", "438100", "--render-percent"],
            &["steam", "play", "438100", "--render-percent", "0"],
            &["steam", "play", "438100", "--render-percent", "101"],
            &["steam", "play", "438100", "--render-percent", "+58"],
            &["steam", "play", "438100", "--render-percent", "58.5"],
            &["steam", "install", "438100", "--language"],
            &["steam", "installed", "--", "--no-vr"],
            &["steam", "login"],
            &["steam", "login", "--qr", "--account", "gordon"],
            &["steam", "login", "--qr", "--guard", "email-code"],
            &[
                "steam",
                "login",
                "--account",
                "gordon",
                "--guard",
                "carrier-pigeon",
            ],
            &["steam", "quit", "438100", "--wait", "soon"],
            &["engine"],
            &["engine", "mend"],
            &["engine", "repair", "now"],
            &["settings", "prereleases"],
            &["settings", "prereleases", "maybe"],
            &["status", "now"],
            &["--library"],
        ] {
            let failure = read(line).unwrap_err();
            assert_eq!(failure.code, USAGE, "{line:?}");
            assert_eq!(failure.status(), 2);
        }
    }

    #[test]
    fn the_help_names_every_command() {
        for command in [
            "status",
            "steam login --qr",
            "steam login --account <name>",
            "steam logout",
            "steam installed",
            "steam install <app_id>",
            "steam play <app_id>",
            "steam running",
            "steam quit <app_id>",
            "engine repair",
            "settings prereleases on|off",
        ] {
            assert!(HELP.contains(command), "{command}");
        }
    }
}
