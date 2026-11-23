# obelisk-cli

`obelisk-cli` is Obelisk's engine without its window. Another program runs it to sign in to Steam, install the Windows build of a game, start it with extra arguments, learn when it runs and when it ended, and stop it. It calls the same Rust the app's Swift calls (`crates/obelisk-bridge`) in the same order, and keeps a library, a Steam session and a cache of its own: it never reads or writes the Obelisk app's library or Keychain items.

This file is the contract a program is written against. Tests in `crates/obelisk-cli` fail when an event or an error code the program can produce is missing here.

## Contents

- [State of this build](#state-of-this-build)
- [Invocation](#invocation)
- [Output](#output)
- [Storage](#storage)
- [Commands](#commands)
- [Events](#events)
- [Error codes](#error-codes)
- [Keyboard layout](#keyboard-layout)
- [Graphics](#graphics)
- [The engine](#the-engine)
- [Questions answered in the player's place](#questions-answered-in-the-players-place)
- [What the app does and this program does not](#what-the-app-does-and-this-program-does-not)
- [Building and signing](#building-and-signing)

## State of this build

0.1.8 makes the engine's attach stop the trace link's own state (never a queued SIGSTOP), puts the engine files in as a recorded transaction and pins a running game's files, bounds a log flood, and records when a Quit was accepted. In detail: `fexdarwin-run` from Orbiters-cc/voile-lkd `bf0af9b`: the stop a `PTRACE_ATTACH` owes its tracer lives in the link, set with it and cleared only when reported to that very link or with it, so no other SIGSTOP, no SIGCONT and no departed tracer can lose it or turn it into a stop of the game (the fourth verification's three reproductions are among its tests), and the engine keeps its last 8192 ptrace and signal events in a ring beside the game's log ([The engine](#the-engine)); `wsd` from `248bf43`: the bundle's own Dock icon comes back when no client with an icon remains. The engine files change only in a transaction whose record (id, phase, every file's SHA-256 before and after) is written before each step, and only while no game runs on the runtime: after a crash, a full disk or a lock that cannot be taken a launch starts on one complete set or is refused (`wine_incomplete`) with a line on standard error naming the path and what to do; the release is checked before anything is recovered ([Keyboard layout](#keyboard-layout)). The Linux engine's launch script writes the guest's output through a bounded writer: a line repeated without end is kept 200 times, then counted, and every line a frame counter reads passes. A Game session record says `quit_accepted`, when its supervisor accepted a Quit. Bridges and other programs that need the whole drawing size can pass `steam play --render-percent 100`: it overrides an installation's saved percentage for that launch.

0.1.7 carries the engine's own program, `fexdarwin-run`, rebuilt from Orbiters-cc/voile-lkd with one change to its ptrace: the SIGSTOP of a `PTRACE_ATTACH` belongs to that trace link and never stops the whole game. Wine's server attaches a game's threads to read or write their debug registers, back to back during a garbage collection, and two orderings turned that SIGSTOP into a job-control stop of every thread of the game, for good: the hang seen on 2026-11-23 (see [The engine](#the-engine)). Its window server keeps the game's icon in the Dock when a helper of the game (VRChat's `yt-dlp.exe`) sends its own. `steam play --render-percent <1..100>` draws a game on the Linux engine at that share of the display's drawing resolution per side, for that launch only. The engine files now replace an earlier build's as they do Voile's own: 0.1.6 found 0.1.5's render servers "other" and left a player on them; `status` tells such a file `ours-earlier`. A failed switch that cannot be put back stops the launch (`wine_incomplete`) instead of starting a game on two sets of files. 0.1.6 makes what 0.1.5 tells hold in the cases that came short. `shown` counts only the log that was there when the game began, by its handle (a log made later at that path is never taken for it, and with no log at the start `shown` is never written); a log cut and grown again between two looks, or rewritten from its start, ends its continuity; and the end count (`played.frames`, `exited.frames`) is read from that same handle, never the path again: unknown (`measured` false, null) when the log lost its continuity or was never there. `exited` is its own launch's session from the first time that session is seen (admitted, before it runs); a launch that never saw its session takes the library's last one only when it began after the launch was asked for. A record's `played.ended` says when the game ended on the clock since the Mac booted, which no change of the wall clock moves. The engine's launch script says when each of its two final waits returned (`obelisk-wait:` lines in the game's log), for a quit that does not finish. The engine files go in all at once or not at all, and `status` tells each one's state (`voile.engine_files`). Neptune's render server takes a resource update whole, within its resource, up to 256 MiB (see [Graphics](#graphics)). Ran on 2026-11-23 through Orbiters - N4N0 0.3.9-dev.27: five VRChat starts; `played.ended` was written on the boot clock; the `obelisk-wait:` lines came 50 ms apart on two clean quits; the owner's avatar loaded and the game drew 6,527 frames over 355 s through the new render server. One start hung 38 s in with the game's whole process in a job-control stop (Orbiters - N4N0's `docs/vrchat-bridge.md`, live run of 0.3.9-dev.27): not this build's change. 0.1.5 says what the game shows while it runs, and only what it knows: the Game session's record carries `shown`, the frames the game has shown so far (see [Game session records](#game-session-records)); `exited.frames` is null when the game's log could not be read, instead of 0; and `exited` is the running session's own, never the record of another launch that became the library's last session meanwhile. Ran on 2026-11-23 through Orbiters - N4N0: `shown` followed four VRChat starts (829 frames when one hung, then stalled; 1,991 and more on the others), and Neptune's render server from 0.1.4 drew through every avatar load. 0.1.4 keeps VRChat's graphics alive under Voile's Linux engine: Neptune's render server refused any one resource update over 64 MiB, and VRChat sends one of 70,744,960 bytes seconds after an avatar loads, after which the game ran on with a frozen picture (sound on, nothing answering). The engine files now carry Neptune's render server too, taking an update up to 1 GiB: see [Graphics](#graphics). 0.1.3 tells which Game session holds the game: `running` and `exited` carry its `session_id`, `exited` the frames the game presented, and `steam quit --session` asks only that session's game to end (`other_session` otherwise), so that a program which started a game never ends one somebody else started meanwhile in the same library. 0.1.2 gives Windows games on Voile's Linux engine the keyboard layout selected on the Mac, for Voile `v26.3.0-27.beta1`: see [Keyboard layout](#keyboard-layout), which also tells what was tested. `status` tells it in `voile.keyboard_layout`. 0.1.1 is 0.1.0 signed with a Developer ID and sent to Apple's notary. It is a first build. `status`, `steam installed`, `settings prereleases`, the command line, every failure before Steam is reached, and `steam login --qr` up to its `challenge` event were run for real. A complete sign-in and `steam install` ran through Orbiters - N4N0 on 2026-11-22. `steam play` with arguments after `--`, `steam running` with a live session, `steam quit --wait` and `steam quit --force` ran against Steam with VRChat that day (0.1.1): the arguments reached the game in order, a quit took 6 s, and a game whose graphics had hung ignored Quit for 60 s and ended only with `--force`. The fields and the option 0.1.3 adds are covered by tests that drive the engine's states.

## Invocation

```text
obelisk-cli [--library <dir>] [--helpers <dir>] <command>
```

| Option | Meaning |
| --- | --- |
| `--library <dir>` | Where games, runtimes, prefixes and Game session records are kept. Default: `~/Library/Application Support/cc.orbiters.obelisk-cli`. A relative path is taken from the working directory. |
| `--helpers <dir>` | The folder that holds `obelisk-steam-host`, `obelisk-steam-sdk-x86_64` and `libobelisk_steam_discovery_adapter.dylib`. Default: `SteamNative` beside the `obelisk-cli` executable, links resolved. `Obelisk Rosetta.app` is looked for beside that folder. |
| `--help`, `-h` | Writes the usage to standard error and exits with 0. |
| `--version`, `-V` | Tells one [`version`](#version) event. |

Both options are accepted anywhere before `--`. Everything after `--` belongs to `steam play`.

Run one `steam install` or `steam play` per library at a time. Obelisk runs one Steam game at a time per library.

## Output

- Standard output carries JSON Lines and nothing else: one JSON object per line, each with an `event` string. Every field listed for an event is always present; a value that is not known is `null`.
- A command that fails tells one [`error`](#error) event last and exits with 1, or with 2 when its command line could not be read (`usage`). A command that succeeds exits with 0. `steam play` succeeds when the game ran and its Game session finished, whatever the game's own exit code, which [`exited`](#exited) carries.
- Standard error carries only `--help`. It is otherwise unused; do not parse it.
- `progress` events are told at most once every 250 ms per phase, and at once when the phase changes.
- A Steam ID is a JSON string: it does not fit the numbers every JSON reader keeps exactly. A Steam app ID and a process ID are JSON numbers.
- Standard input is read only by `steam login --account`.
- A reader that closes standard output loses the events and stops nothing.

## Storage

| What | Where |
| --- | --- |
| Library: games (`Games/Steam`), Voile runtimes (`Voile`), Wine prefixes (`Prefixes`), Cloud archive (`SteamCloud`), launch settings (`LaunchSettings`), game profiles (`profiles.json`) | `--library`, default `~/Library/Application Support/cc.orbiters.obelisk-cli` |
| Installation receipts of the account | `<library>/steam-installations-<steam id>.json` and `.incomplete.json` |
| Game session records of the supervisors | `<library>/SteamSessions/session.v2.json` and `<session id>.v2.json` |
| Steam session (account name, Steam ID, refresh token) | Login Keychain, generic password, service `cc.orbiters.obelisk-cli.steam`, account: the library's path. Never in a file, never in an event. |
| Valve's Steam components, downloaded from Valve's CDN | `~/Library/Caches/cc.orbiters.obelisk-cli/SteamRuntime` |
| Whether Voile's pre-releases are taken | `<library>/Voile/prereleases` (an empty file) |
| Game logs and launch reports | `~/Library/Logs/Obelisk/Steam`, such as `wine-<app id>.log`. The engine names this folder from the home directory: the Obelisk app writes its logs there too. |

Each library has its own account: the Keychain item is named after the library's path.

The Keychain item belongs to the signature of the program that saved it. A build signed with another identity, or an unsigned one, may neither read, replace nor delete it: no question is ever shown, and every command that needs the session, `steam login` and `steam logout` included, fails with `keychain_unavailable`. Delete the item, then sign in again with that build:

```sh
security delete-generic-password -s cc.orbiters.obelisk-cli.steam -a "<library directory>"
```

The engine reads Voile's signed feed from `Zagrios/Voile` and the game profiles from `Zagrios/Obelisk` on GitHub as a Windows game launches, and downloads Voile from that repository's releases.


### Game session records

A Game session's record, `<library>/SteamSessions/<session id>.v2.json`, is its supervisor's journal, rewritten as the session changes; this program reads it and so may another. What a reader may rely on:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | The Game session's UUID, the file's name. |
| `store.app_id` | number | The Steam game. |
| `phase` | string | `Preparing`, `Running`, `Finalizing`, `Finished` or `Failed`. |
| `game.pid` | number | While the game runs: the process the supervisor started for it (for a Windows game, the engine's launcher). |
| `played.seconds`, `played.frames.count` | numbers | Once the game exited: how long it ran, and the frames it presented in all. |
| `played.frames.measured` | boolean | False when the count is not known, and then `count` (0) says nothing: from 0.1.5 when the game's log could not be read; from 0.1.6 also when the log was not there when the game began, or lost its continuity (see `shown`). True with `count` 0 for a log read whole in which Metal reported no frame: a game whose graphics never showed anything. Absent in older records, which read the log the same way: count them as `true`. |
| `played.ended` | object or absent | From 0.1.6: when the game ended on the clock since the Mac booted: `boot` (string), that boot's `kern.bootsessionuuid` in upper case; `seconds` (number), whole seconds since that boot, sleep counted (`CLOCK_MONOTONIC`). A wall clock set forward or back never moves it; another `boot` is another boot. Absent where it could not be read, and in older records. |
| `quit_accepted` | object or absent | From 0.1.8: when the session's supervisor accepted a Quit or Force Quit of the running game, on the boot clock (`boot`, `seconds`, as `played.ended`). Written before the quit's work begins: a quit that does not finish is told from it whatever the game or its window server can still log. |
| `shown` | object or absent | From 0.1.5, while a game whose launch writes a log runs: `frames` (number), the frames it has shown so far, read from that log every 2 s and written when it changes, else at least every 10 s; `at` (number), when that was, in seconds since 1970; `continuous` (boolean), the log was read whole from the game's start: false once it was cut shorter, rewritten from its start (0.1.6: its first 4 KiB checked at each look), replaced at its path or could no longer be read, after which `frames` stays where it was and tells nothing more of the game's graphics. From 0.1.6 absent for the whole session when there was no log to open as the game began (the count is not known). A game whose graphics stopped shows no new frame: `frames` stops growing while `at` goes on. Kept as it was last written once the game exited. |

The log is the one the game's launch writes Metal's frame reports to (for Steam on the Linux engine, `~/Library/Logs/Obelisk/Steam/wine-<app id>.log`, a new file at each launch, the previous one kept as `wine-<app id>.previous.log`). From 0.1.6 it is opened once, as the game begins, and that handle is the only file `shown` and `played.frames` count: a later launch's log at the same path is never counted for this session, and the end count is read again from that handle, not the path. A session's `shown` belongs to that session alone; it says nothing of whose game it is (the record's `game` does).
## Commands

### `status`

Tells one [`status`](#status-1) event: the signed-in account, the Voile release installed, whether pre-releases are taken, and the helpers found or missing. It reaches no network and makes nothing, not even the library.

### `steam login --qr`

Tells [`challenge`](#challenge) with the URL to show as a QR code, again each time Steam replaces it, then [`signed_in`](#signed_in) once the sign-in is approved in the Steam mobile app, and saves the session in the Keychain. It ends with `login_expired` when nobody approved it in time.

### `steam login --account <name> [--guard <kind>]`

Reads the password from the first line of standard input. When Steam asks for a confirmation, tells [`guard`](#guard), reads the code from the next line of standard input when `needs_code` is true, then tells [`signed_in`](#signed_in) and saves the session.

`--guard` picks the confirmation among those Steam offers: `device-confirmation`, `device-code`, `email-code` or `email-confirmation`. Without it, the first of these, in this order, that Steam offers, as the app picks: the phone's approval before a code. A kind Steam does not offer fails with `guard_unsupported`.

### `steam logout`

Forgets the saved session and tells [`signed_out`](#signed_out). It fails with `game_running` while a game of the account runs, which keeps the session it started with: quit it first. This check needs `obelisk-steam-host`; without it, the session is forgotten unchecked.

### `steam installed`

Tells one [`installation`](#installation) event per installed game of the signed-in account whose folder is there, then [`done`](#done). It reads the account's receipts and reaches no network. Signed out, it tells `done` with a count of 0.

### `steam install <app_id> [--language <locale>]`

Installs the Windows build of the game, with what a launch needs: Valve's Steam components and a Voile release, each downloaded before the game's files when it is missing. It tells [`preparing`](#preparing), [`progress`](#progress) events, then [`installed`](#installed).

It runs the game's Transfer through the app's download queue whatever is installed. Over an installed game, the Transfer reads Steam's current build, verifies the installation against it and repairs or updates it in place: it downloads nothing when the build is current. An interrupted Transfer resumes.

`--language` is a locale such as `fr-FR`, whose Steam language the game's depots are installed in; without it, the locale `LC_ALL` or `LANG` names, else English. An installed game keeps the language it was installed in.

It needs a signed-in account and `obelisk-steam-host`, and ends with `busy` while a game runs.

### `steam play <app_id> [--render-percent <1..100>] [-- <argument>...]`

Starts the installed Windows build as the app's Play does and stays attached until its Game session is finished:

1. [`preparing`](#preparing).
2. [`progress`](#progress), while a Transfer repairs an interrupted installation or installs Steam's newer build, and while a runtime that is missing or outdated downloads. [`voile_update`](#voile_update) when a newer Voile release is taken.
3. [`starting`](#starting), once the game's supervisor holds it.
4. [`running`](#running), with the game's process ID.
5. [`cloud_pending`](#cloud_pending), only when Steam Cloud asks a question after the game exited.
6. [`exited`](#exited), with the game's exit code.

`--render-percent` (from 0.1.7) has a game on Voile's Linux engine drawn at that share, per side, of the display's drawing resolution (100: as Voile draws it, the display's backing pixels): the engine's window server tells the game a smaller monitor and macOS scales the picture to the screen. For this launch only, over the game's own launch settings (`render_percent`), never kept. It must come before `--`; a value that is not a whole number from 1 to 100 is a `usage` failure. A game on Rosetta's Wine, whose engine offers none, ignores it. The window server writes what it applied in the game's log (`wsd: render percent: requested <n>, applied <n>; ... (desktop <w>x<h> px)`).

The arguments after `--` start the game after Steam's own, the game profile's and those kept in its launch settings, where the app's Launch Settings arguments go. Each is passed as it is given, never split or joined, and none is kept: the next launch does not have them. They are written to the head of the game's log, so pass no secret there.

From `starting` on, the supervisor (`obelisk-steam-host`, in its own session) owns the game: the game runs on when `obelisk-cli` is killed, and `steam running` and `steam quit` find it. Before `starting`, the launch lives in this process: killing it abandons the launch, and a Transfer it interrupted resumes at the next `steam play` or `steam install`.

It fails with `not_signed_in`, `helpers_missing`, `not_installed`, `already_running` when a game of that app runs, `another_game_running` when another Steam game does, or one of the launch failures [below](#error-codes).

### `steam running`

Tells one [`session`](#session) event for the Game session that holds Steam in this library, if there is one, then [`done`](#done). It needs `obelisk-steam-host`, which tells whether the recorded processes still live.

### `steam quit <app_id> [--session <id>] [--force] [--wait <seconds>]`

Asks the running game to end, as the app's Quit does: it is asked to close its windows, and nothing is killed. A launch still preparing under its supervisor is cancelled. `--force` is the app's Force Quit: the game ends now, with whatever still runs for its installation, and its session still settles its Cloud saves.

It tells [`quit_requested`](#quit_requested) once the supervisor took the request. With `--wait`, it then waits that many seconds for the Game session to end and tells [`exited`](#exited), or fails with `still_running`. A game may answer Quit with a question of its own and play on.

With `--session`, only the game of that Game session (the `session_id` of its [`running`](#running) event) is asked to end: when another session of the app runs, it fails with `other_session` and nothing is asked.

It fails with `not_running` when no game of that app runs.

### `settings prereleases on|off`

Keeps whether Windows games run in Voile's pre-releases too, as the app's beta releases setting does, and tells [`settings`](#settings). Turned on, the next `steam install` that misses Voile installs the newest release or pre-release, and the next `steam play` takes it over an older one. Turned off, a pre-release in place is left for the newest release.

## Events

### `version`

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | string | This program's version, such as `0.1.0`. |

### `status`

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | string | This program's version. |
| `library` | string | The library directory. |
| `account` | object or null | The signed-in account: `steam_id` (string) and `account_name` (string). Null when signed out, or when the Keychain did not answer. |
| `keychain` | string | `ok`, or `unavailable` when the Keychain could not be read. |
| `voile` | object | `installed` (string or null): the Voile release Windows games run in now, such as `v26.3.0-16`. `prereleases` (boolean): pre-releases are taken. `keyboard_layout` (string or null): whether this build's engine files are in place for the games of the Linux engine, told by the keyboard they get, see [Keyboard layout](#keyboard-layout) and [Graphics](#graphics): `mac` (all in place: the layout selected on the Mac, Neptune's render servers of this build), `pending` (the release they are for, with Voile's own files or some of ours left by an older build: all of ours from the next launch on the engine), `us` (another release, or a file that is neither Voile's nor ours: none is changed, and its games get a US keyboard whatever the Mac's layout, and whatever render servers are there). Null when no release with the Linux engine is installed. `engine_files` (array or null, from 0.1.6): each engine file of this build, `path` (in the Linux engine) and `state`: `voile` (the release's own), `ours` (this build's), `ours-earlier` (from 0.1.7: a file an earlier build of this program put there, replaced at the next launch on the engine like the release's own), `other` (neither), `unreadable` (from 0.1.8: there but it cannot be read; no game starts until it can): what is in place now, file by file, whatever `keyboard_layout` sums up. Null without a Linux engine. |
| `helpers` | object | `directory` (string), `found` (boolean: every helper a launch needs is there), `missing` (array of file names), `rosetta_helper` (boolean: `Obelisk Rosetta.app` is beside the folder). |

### `challenge`

| Field | Type | Meaning |
| --- | --- | --- |
| `qr_url` | string | The URL to show as a QR code. A later `challenge` replaces it. |

### `guard`

| Field | Type | Meaning |
| --- | --- | --- |
| `kind` | string | The confirmation the sign-in waits for: `device_confirmation`, `device_code`, `email_code` or `email_confirmation`. |
| `needs_code` | boolean | A code is read from the next line of standard input (`device_code`, `email_code`). Otherwise the sign-in waits for the approval. |
| `methods` | array | Every confirmation Steam offers: `kind` (the kinds above, or `none`, `machine_token`, `unknown`) and `message` (string, what Steam says of it, such as the e-mail's domain; may be empty). |

### `signed_in`

| Field | Type | Meaning |
| --- | --- | --- |
| `steam_id` | string | The account's Steam ID. |
| `account_name` | string | The account's name. |

### `signed_out`

| Field | Type | Meaning |
| --- | --- | --- |
| `was_signed_in` | boolean | An account was signed in. |

### `installation`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game's Steam app ID. |
| `name` | string | The game's name. |
| `directory` | string | Its installation folder. |
| `platform` | string | `windows` or `mac`. |
| `bytes` | number | The installation's size. |
| `needs_verification` | boolean | A Transfer started over it and has not succeeded since: `steam play` repairs it first. |

### `done`

| Field | Type | Meaning |
| --- | --- | --- |
| `count` | number | How many `installation` or `session` events came before it. |

### `preparing`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game the command now signs in for. |

### `progress`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `phase` | string | `preparing` (Steam is asked what to install), `steam_components` (Valve's Steam components download), `voile` (a Voile release downloads), `verifying` (the game's files are read), `downloading` (the game's files download), `moving` (the installation moves into the games folder). |
| `bytes_done` | number or null | Bytes of the phase done. Null while a runtime downloads for `steam play`, which tells only a fraction. |
| `bytes_total` | number or null | Bytes of the phase in all; 0 while it is not known yet. Null with `bytes_done`. |
| `fraction` | number or null | From 0 to 1. Null while the total is not known. |

### `voile_update`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game being started. |
| `version` | string | The newer Voile release its launch takes. |
| `required` | boolean | The game's profile needs it. |
| `answer` | string | Always `update`. |

### `cloud_pending`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game that exited. |
| `kind` | string | `conflict` (the saves here and in Steam Cloud differ) or `failed` (the sync failed). The sync was cancelled: the saves stay on this Mac, and the Cloud keeps its own, until the player settles it in Obelisk. |

### `installed`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `name` | string | Its name. |
| `directory` | string | Its installation folder. |
| `platform` | string | `windows`. |
| `bytes` | number | The installation's size. |
| `voile` | string or null | The Voile release Windows games run in now. |

### `starting`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game its supervisor now holds. |

### `running`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `pid` | number or null | The process the supervisor started for the game, as its record names it. For a Windows game it is the engine's launcher, not a Windows process. Null when the record could not be read, or when the game exited before it was seen running. |
| `session_id` | string or null | The Game session that holds the game, a UUID: the name of its record `<library>/SteamSessions/<session id>.v2.json`, and what `steam quit --session` takes. Null when the game exited before its session was seen. |

### `exited`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `session_id` | string or null | The Game session that held the game: from 0.1.6 this launch's own, from the first time its session was seen; a launch that never saw it takes the library's last session only when that began after the launch was asked for. Null when it is not known. |
| `frames` | number or null | How many frames the game presented to the screen in all, as its Game session counted them (`played.frames`): 0 for a game whose log was read whole without a frame reported (its graphics never showed anything). Null when it is not known: the game's log was missing or could not be read (from 0.1.5; 0.1.4 and before said 0), was not there as the game began or lost its continuity (from 0.1.6), or the session's record could not be read. |
| `exit_code` | number or null | The game's exit code. Null when a signal ended it or when it is not known. |
| `exit_signal` | number or null | The signal that ended it, if one did. |
| `seconds` | number or null | How long it ran. |
| `ended_by_request` | boolean | The game was asked to end through its supervisor, such as by `steam quit`. |

### `session`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `steam_id` | string | The account it runs for. |
| `session_id` | string | The Game session's ID, a UUID. |
| `phase` | string | `preparing`, `running`, `finalizing` (the game exited and its session settles), `finished` or `failed`. |
| `pid` | number or null | As in [`running`](#running). |
| `reconnecting` | boolean | The supervisor could not be reached; the game may still run. |

### `quit_requested`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
| `forced` | boolean | It was a Force Quit. |

### `settings`

| Field | Type | Meaning |
| --- | --- | --- |
| `prereleases` | boolean | Voile's pre-releases are taken. |

### `error`

| Field | Type | Meaning |
| --- | --- | --- |
| `code` | string | One of the [error codes](#error-codes). |
| `message` | string | A sentence in English for a log. Do not parse it. |
| `key` | string or null | Obelisk's own reason when the engine gave one: a localization key of the app, such as `backend.steam.runtime.startFailed`. It tells failures apart under one code. |

## Error codes

An engine reason with a code of its own keeps it whatever the command was doing; any other takes the code of what was being done (`login_failed`, `steam_failed`, `install_failed`, `launch_failed`, `session_failed`, `quit_failed`) and is named in `key`.

| Code | Meaning |
| --- | --- |
| `usage` | The command line could not be read. Exit status 2. |
| `internal` | Something this program did not expect, such as a receipts file it cannot read. |
| `library_unavailable` | The library directory cannot be made or used, or the home or working directory is unknown. |
| `helpers_missing` | `obelisk-steam-host`, or for `steam play` any helper, is not in the helpers' folder. |
| `keychain_unavailable` | The login Keychain did not give or take the Steam session: it is locked, or the item belongs to another signature. See [Storage](#storage). |
| `not_signed_in` | No Steam account is signed in. |
| `sign_in_expired` | Steam asks the account to sign in again. The saved session was forgotten. |
| `input_missing` | Standard input gave no password, or no Steam Guard code. |
| `credentials_rejected` | Steam refused the account name or password. |
| `guard_rejected` | Steam refused the Steam Guard code. |
| `guard_unsupported` | Steam does not offer the confirmation `--guard` names, or offers only ones this program cannot give. |
| `login_expired` | The sign-in expired before it was approved. |
| `rate_limited` | Steam asks to wait before another sign-in. |
| `login_failed` | The sign-in failed for the reason in `key`. |
| `network` | Steam could not be reached. |
| `steam_failed` | Steam refused or failed a request, for the reason in `key`. |
| `not_owned` | The account does not own the game. |
| `no_windows_build` | The game has no Windows build the account can install. |
| `not_installed` | The Windows build is not installed, or its download did not finish. |
| `busy` | Another operation holds the game: a game runs, a Transfer of the game runs, or another process installs it. |
| `already_running` | A game of that app already runs. |
| `another_game_running` | Another Steam game runs in this library. |
| `game_running` | `steam logout` while a game of the account runs. |
| `not_running` | `steam quit` while no game of that app runs. |
| `session_recovery_failed` | The Game session a previous run left could not be reached within 20 seconds. |
| `cancelled` | The Transfer or the launch was cancelled before it ended. |
| `install_failed` | The installation failed for the reason in `key`, or in `message` for the games folder. |
| `launch_failed` | The launch failed before the game started, for the reason in `key`. |
| `session_failed` | The Game session failed after the game ran, for the reason in `key`. It follows `exited`. |
| `rosetta_required` | The game runs on Rosetta's Wine or renders with D3DMetal, and Rosetta is not installed. |
| `cloud_conflict` | The saves on this Mac and in Steam Cloud differ before the game starts. The launch was cancelled. |
| `cloud_sync_failed` | Steam Cloud could not be checked before the game starts. The launch was cancelled. |
| `voile_failed` | A Voile release could not be downloaded, read or stored. |
| `steam_components_failed` | Valve's Steam components could not be downloaded, read or stored. |
| `quit_failed` | The supervisor did not take the request to end the game. |
| `still_running` | `steam quit --wait`: the game still runs after that long. |
| `other_session` | `steam quit --session`: the game of the app runs in another Game session than the one named. Nothing was asked to end. |
| `settings_failed` | The setting could not be kept in the library. |

## Keyboard layout

Voile's Linux engine, which runs VRChat and the other games whose profile names it, takes every Mac keyboard for a US one: its window server (`wsd`) names a key by its place, and its Wine driver (`wineobelisk.drv`) has no keyboard layout, so on an AZERTY Mac the key that types "a" gave the game `VK_Q` and "q". Games on Rosetta's Wine are not concerned: CrossOver's Mac driver reads the layout itself.

`obelisk-cli` carries two engine files of its own, inside the program: `wsd` and `wineobelisk.so`, built from Voile's sources plus the layout ([Orbiters-cc/voile-lkd](https://github.com/Orbiters-cc/voile-lkd) and [Orbiters-cc/voile-wine](https://github.com/Orbiters-cc/voile-wine), branch `orbiters`; the commits are in `crates/game-runtime/engine-patch/manifest.json`). As a game starts on the Linux engine, they replace the two files of the Voile release in the library, at `linux/engine/wsd` and `linux/wine/lib/wine/x86_64-unix/wineobelisk.so`. With them:

- A key has the scan code of its place, and the virtual key a PC keyboard of the Mac's layout gives it: on a French layout the key right of Tab is `VK_A`, the digits row keeps `VK_1` to `VK_0`, and the punctuation keys have the virtual keys of Windows' French layout. Letters and digits are right in every layout; the `VK_OEM_*` keys follow the rules of Wine's Mac driver, checked against Windows' French and German layouts.
- Typed text (`WM_CHAR`, `ToUnicodeEx`) is what the Mac's layout types, with Shift, Caps Lock as the Mac's layout defines it, and dead keys (`^` then `e` types "ê").
- Where the layout types ASCII characters with Option, as French and German do for braces, the right Option key is AltGr, sent as left Control and right Alt as Windows sends it, and types those characters. The left Option key stays Alt. A layout that types all of ASCII without Option (US, Dvorak) keeps two Alt keys.
- Picking another input source while the game runs changes the layout at once, and the game's window gets `WM_INPUTLANGCHANGE`.

What they do not do: the left Option key types no Option character; `GetKeyboardLayout` and the layout's name still follow the Windows locale, not the Mac's input source; a key that types more than one UTF-16 unit types nothing; a dead key pressed after another dead key types both accents instead of chaining; an input method (Japanese, Chinese) gives its Latin layout, not its composition.

The files are for one Voile release, `v26.3.0-27.beta1` in this build, and go over the files that release shipped and no others, told by their SHA-256. Any other release, a development runtime, or a file that is neither Voile's nor ours is left untouched, and its games get the US keyboard as before: `status` then tells `us`. When a launch takes a newer Voile release, the layout is lost until a build of `obelisk-cli` carries files for it. A runtime that cannot be written stays as it is and the game starts all the same.

From 0.1.8 the files change in a recorded transaction and never under a running game. The runtime has two locks: `linux/.obelisk-engine-patch.lock`, held by whoever looks at or changes the files, and `linux/.obelisk-engine-sessions.lock`, held shared by every launch from its preparation to its game's exit (the preparing program holds it until the session's supervisor holds its own, which it keeps until it ends). A launch changes the files only when it can take the sessions lock exclusively: with a game running on the runtime it starts on the files as they are (`pending` then), which are the running game's. The transaction writes its record, `linux/.obelisk-engine-patch.txn.json` (an id, the phase `staging`, `switching` or `committed`, the Voile release, and each file's exact SHA-256 before and after), synced, before each phase; ours are written beside the files as `.obelisk-txn-<id>-new-<name>`, each original linked aside as `.obelisk-txn-<id>-old-<name>` (named by the transaction and the destination), all switched, every file checked, then `committed`. At the next launch a record left by a crash is ended from it alone: committed, only its kept originals are removed (a failure to remove them costs nothing, and the update is never undone); any other phase puts every file back to its `before`, each from the original kept for that transaction and that file and checked by its digest. The release is checked before anything is recovered: a record for another release than the runtime's now, a record that cannot be read, a file or a kept original that cannot be read (told apart from one that is someone else's), or one that is not the digest the record names, refuses the launch with `wine_incomplete` and a line on standard error, `engine patch: not started: <what is wrong> (<paths>)`, which says to reinstall Voile's release or to remove the record and the `.obelisk-txn-*` files once checked; nothing is changed by a refusal. A full disk at any write leaves the files as they were (the launch runs on them); a runtime whose lock cannot be made (not writable) starts only on a set it can read whole with no record pending. The originals an older build kept aside (`.obelisk-original-<name>`) are never installed over a file of the set; only the sole copy of a file that is gone is put back. Why not a separate runtime folder per generation: the runtime's path is part of the Wine prefix's state and of what the game and its anti-cheat see, and moving every session to a new folder at each update could not be soaked in the few starts a Mac may make; the pin gives a running session the same guarantee (nothing it uses changes) without moving anything.

From 0.1.7 a file that an earlier build of this program put in place counts as one to replace, like the release's own: the manifest lists, for each path, every SHA-256 an earlier build put there (`previous`), and `scripts/build-engine-patch.sh` writes no manifest that leaves one out. A kept original goes back only when it is the release's file or an earlier one of ours (another file under that name is removed, never installed), and temporaries a crash left beside the files are removed. A switch that fails, and whose undo fails too, leaves files of two sets: the launch is refused with `wine_incomplete` (and the paths written to standard error) rather than a game started on them; the next launch tries again from what was kept aside.

From 0.1.6 the files go in as one, before a launch on the engine: under a lock of the runtime (`linux/.obelisk-engine-patch.lock`), the files are looked at once, each of ours is written beside the file it replaces and checked by its SHA-256, each original is kept aside (`.obelisk-original-<name>` beside it), then all are switched; a switch that fails part way puts every original back, and one a crash cut short is undone at the next launch before anything else. Older builds switched file by file: a runtime they left with some of each is `pending`, and the next launch puts all of this build's in.

Tested: the layout code against the Mac's own US, French, French - PC, German, Dvorak, ABC - AZERTY and Russian layouts (`tests/keyboard` in voile-lkd, 569 checks); and a Windows program run through the patched engine of a copy of `v26.3.0-27.beta1`, with keys injected by `wsdctl`, under French - PC, French, German and US layouts and across a change of input source: scan codes, virtual keys, raw input, characters, AltGr, dead keys, `MapVirtualKey`, `VkKeyScan` and `GetKeyNameText`. Never run: a key from a real keyboard, and a game.

## Graphics

Windows games on Voile's Linux engine draw through Neptune: its guest driver (`d3d11.dll` in the game's process) sends each Direct3D call over a command stream to a render server on the Mac (`virgl_render_server`), which replays it with DXMT (arm64) or D3DMetal (x86_64, under Rosetta). The render server refused one call, a resource update (`UpdateSubresource`, or a texture's or buffer's first data) larger than 64 MiB, and a refused command fails the guest's command ring for good: the engine's log says `virgl_render_server: npt: resource_update: byte_size <n> exceeds cap 67108864`, then `neptune: ring fatal error` and `VTEST_CLIENT_ERROR_CONTEXT_LOST`, and the game runs on without a new frame. VRChat sends one of 70,744,960 bytes about 5 s after the local avatar loads (seen 2026-11-22 on three starts, two through this program and one through Orbiters - N4N0's Home, full screen and windowed).

An update's bytes come inline in the command stream; anything larger than the ring's direct part is copied by the guest into its own upload buffer, grown to fit, and sent as `EXECUTE_COMMAND_STREAM` over it, which the server checks against that buffer's size before it reads. 0.1.4 raised the cap to 1 GiB on that ground. 0.1.6's render server (neptune `92f928c`) takes an update only when all of this holds, and refuses it otherwise with `npt: resource_update: ...` in the engine's log:

- the command is whole before anything is allocated or read for it: its `cmd_size` is its header and its payload rounded to 8, and all of that is in the stream;
- `byte_size` is at most 256 MiB: what the whole path was tested to (below), 3.8 times VRChat's 70,744,960-byte upload;
- a payload copied to the decoder's temporary buffers (one not contiguous in the stream) counts against 512 MiB for the whole server process at once;
- for Direct3D 11, the bytes `UpdateSubresource` reads, computed from the resource's own description (a buffer's `ByteWidth`; a 1D, 2D or 3D texture's size at the subresource's mip, its DXGI format's block size, the box and the pitches), are at most `byte_size`; a box outside the subresource, one that does not start on a block, a row pitch shorter than a row, a subresource past the resource, or a format whose layout the server does not know (the video formats but AYUV, Y410, Y416, NV12, P010, P016, YUY2, Y210, Y216) is refused. A refused update leaves the resource as it was and the stream goes on.

Not changed: Direct3D 12's `WriteToSubresource` is passed the box and pitches without that footprint check; and the guest driver is Voile's own (it is not among the engine files): its upload buffer starts at 1 MiB and doubles to fit (128 MiB for the 70,744,960-byte upload), Direct3D 11's initial texture data skips updates over 64 MiB, and Direct3D 12's guest refuses them.

`obelisk-cli` carries the render servers of [Orbiters-cc/neptune](https://github.com/Orbiters-cc/neptune) (branch `orbiters`, Voile's commit plus this change; GPL-3.0 for our change, MIT for Neptune's code) and puts them over the release's own as it does the files of [Keyboard layout](#keyboard-layout), at `linux/graphics/libexec/virgl_render_server` and `linux/graphics/x86_64/libexec/virgl_render_server`, under the same rules: for `v26.3.0-27.beta1` only, over the files that release shipped only, one patch for all four files, put in as one (see [Keyboard layout](#keyboard-layout)). The game's helper bundle takes a fresh copy of the server at each launch. Nothing of the guest driver changes.

Tested: the unchanged sources rebuild the release's two render servers byte for byte but for the linker's UUID and the ad hoc signature over it (`scripts/build-engine-patch.sh` refuses to write a patch otherwise). `scripts/test-resource-update.sh` in the fork sends resource updates through the server's own decoder and dispatch into a stand-in Direct3D 11 texture whose description it sets: 4 KiB, 64 MiB, 70,744,960 bytes and 256 MiB arrive whole, in place; refused with nothing called: a payload short of the stream (and 256 MiB with half of it missing, before any temporary allocation), one byte over the cap, `UINT32_MAX`, a `cmd_size` that is not the payload's, a texture one row taller than the bytes sent, a row wider than them, an unknown format, a subresource past the texture's. On 2026-11-22 VRChat started through 0.1.4 into its owner's home world, his 64 MB avatar loaded, and it went on drawing: 431 s, 4,594 frames, no cap line and no lost context, until the Mac's screen was locked and macOS stopped showing the window, then a graceful quit by session in 7 s. The x86_64 server is built and checked the same way and was never run (the Mac tested has no Rosetta).

## The engine

0.1.7 replaces `linux/engine/fexdarwin-run` (FEXCore and LKD, the Linux system calls on macOS) and again `linux/engine/wsd`, built from [Orbiters-cc/voile-lkd](https://github.com/Orbiters-cc/voile-lkd) (branch `orbiters`, Voile's commit `7c7d977` plus ours; GPL-3.0), under the same rules as the files of [Keyboard layout](#keyboard-layout): for `v26.3.0-27.beta1` only, over the files that release shipped or an earlier build of this program put there, one patch for all five files.

**The hang.** On 2026-11-23 a VRChat start stopped 38 s in for good: every guest thread of the game sat in LKD's job-control stop (`park()`, untraced), nothing ran, nothing resumed it, and only a forced quit ended it. Wine's server reads and writes a thread's debug registers through ptrace (attach, wait for the attach's SIGSTOP, `PEEKUSER`/`POKEUSER`, detach); once a thread's debug registers were set, every `GetThreadContext` of it does it again, so a garbage collection that inspects every thread attaches and detaches back to back. In Linux the attach's SIGSTOP is always seen by a traced task. In LKD two orderings let the tracee take it as untraced, whose default action stops the whole process: the tracee, leaving the previous stop, read "no link" from the shared table just before the next attach linked it, and wrote that stale state over the flag the tracer's wake had just set; or the tracer ended after queuing the SIGSTOP and before the tracee took it. Both are reproduced without a game in voile-lkd's own tests (`lkd/tests/test_ptrace_reattach.c`, Wine's sequence at speed between two processes, failed on the release's code in 4 of 4 long runs; `test_ptrace_transport.c`, a two-thread tracee with the attach's flag update withheld, and with the tracer ending between the two), and pass on ours: the attach's SIGSTOP carries its link (slot and generation) and is reported to that link's tracer while it lives, dropped with it otherwise; LKD's flag sync reads the table again after writing; any other SIGSTOP keeps its job-control meaning (a control test). LKD also logs, for the next incident: the first attaches and any whose wake or SIGSTOP failed (tracer, target, link, generation), every group stop with its signal, sender and what ptrace knew, every SIGCONT that resumes one, and every attach stop dropped (`lkd: ptrace:` and `lkd: sig:` lines in the game's log). Not yet proven against a live game when this build was made: the next VRChat starts on it are its soak.

**0.1.8: the attach stop, done as a link's state.** A verification of 0.1.7 reproduced three more ways the attach's SIGSTOP, still a queued signal, could go wrong: a second tracer's SIGSTOP coalesced with a departed tracer's and dropped with it, an ordinary SIGSTOP coalesced the same way, and a SIGCONT to another thread flushing it. In `bf0af9b` the stop is part of the link (set in the same critical section as the link, cleared only when reported to that generation of it, when a SIGSTOP stop of the same link takes its place as in Linux, or with the link); the tracee is told by a hint the tracer's wake (bound to the link's slot and 64-bit generation) and every flag sync set while a stop is pending, and takes it before any queued signal, deciding from the link itself. The flag sync reads the table and writes the tracee's flags under one lock; an attach whose wake cannot be delivered is undone (ESRCH); a tracer killed between linking and waking leaves its tracee running, and the next attach reclaims the link; a parked tracee of a killed EXITKILL tracer dies after detaching; clone reads the parent's link and makes the child's together; a non-leader exec ends a link made to the leader's tid in its rename window. **The event ring:** LKD writes every attach, attach stop, detach, cancelled link, group stop and SIGCONT resume, in order, to `LKD_EVENT_RING`, which the launcher sets to the game's log's name with the extension `.lkd-events` (for Steam, `~/Library/Logs/Obelisk/Steam/wine-438100.lkd-events`), new at each start: a header line, then 8192 fixed lines of 128 bytes, the newest over the oldest; sort the lines after the first to read them in order. The game's log keeps only a few of these lines. **The log flood:** after a renderer's graphics went for good, Neptune's guest driver retried without end and wrote 420 MB in five minutes; the launch script now writes the guest's output through a bounded writer (`awk`, in `launch.sh`): a line (numbers aside) is kept 200 times, then a count at 1,000, 10,000, 100,000 and every million, and the totals at the end; lines with `neptune unixlib wsi`, `guest swapchain`, `metal-HUD`, `wsd:`, `lkd:`, `obelisk-wait:` or a lost renderer's always pass, and the log is never cut or rewritten (the frame counters keep their continuity). The driver itself (inside the game's process) is unchanged.

**The Dock icon.** Each Wine program sends its icon to the window server as it attaches, and the last one stayed: VRChat's `yt-dlp.exe`, started by a world's video player, put its own in the Dock. The window server now shows the icon of the client whose window first got a swapchain's plane (the game) until it disconnects, before that the one whose window was shown first, and never one without a window over one with a window (`tests/graphics/run-wsd-icon.sh`, two synthetic clients; the window server logs `application icon shown: ...`).

Tested: the release's own `fexdarwin-run` rebuilt from its sources byte for byte but for the linker's UUID and the ad hoc signature (`scripts/build-engine-patch.sh` refuses to write a patch otherwise); LKD's ptrace and signal tests on ours; a guest shell run through it with a job-control stop and continue; the files of every earlier build (`cli-v0.1.2` to `cli-v0.1.6`) over a copy of the release as Voile ships it are replaced by these (`every_earlier_release_s_files_are_replaced`, run with `OBELISK_PRISTINE_RELEASE`).

## Questions answered in the player's place

Where the app asks its player, a command answers at once and tells what it did.

| The app asks | This program |
| --- | --- |
| A newer Voile release is offered before the game starts: take it, skip it, or cancel | Takes it, and tells `voile_update`. |
| The game needs Rosetta, which is not installed: install it? | Declines, and fails with `rosetta_required`. Install it with `softwareupdate --install-rosetta`. |
| Steam Cloud and this Mac hold different saves before the game starts: keep which? | Cancels the launch, and fails with `cloud_conflict`. No save is replaced. |
| Steam Cloud could not be checked before the game starts: retry, launch anyway, or cancel | Cancels the launch, and fails with `cloud_sync_failed`. |
| Steam Cloud asks after the game exited | Cancels the sync, tells `cloud_pending`, then `exited`. |
| Steam offers both a Mac and a Windows build | Installs and plays the Windows build. |

Rosetta is needed by a game that runs on Rosetta's Wine, which is every Windows game without a profile or Easy Anti-Cheat, and by a game that renders with D3DMetal on the Linux engine. A game on the Linux engine with DXMT, such as VRChat, needs none.

## What the app does and this program does not

- It ends no orphaned Windows process as it starts (`end_orphaned_windows_games`): a game its supervisor still owns may run from the library's Voile.
- It shows no overlay: a running game's requests for Steam's overlay, its invite dialog, an invitation a friend sent and an in-game purchase to confirm are left unanswered.
- It keeps no library cache, launch history or performance report, and no chat or friends list.
- It never installs newer builds on its own between launches, moves the games folder, uninstalls a game, or syncs Cloud saves on request.
- It asks Steam for a newer build only as a launch or an install starts, not when it signs in.
- `steam logout` does not revoke the account from a running game; it refuses while one runs.

## Building and signing

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./scripts/build-cli.sh
```

The script needs Apple silicon, Xcode's command-line tools and both Rust targets. It builds `obelisk-cli` in release for `aarch64-apple-darwin`, and the Steam helpers as `scripts/build-steam-native.sh` builds them for the app, without Xcode's environment: `obelisk-steam-host` for Apple silicon, `obelisk-steam-sdk` for Intel, the discovery adapter for both in one library, and the Intel-only `Obelisk Rosetta.app`.

It writes `build/dist/obelisk-cli-<version>-arm64.tar.gz` and `obelisk-cli-<version>-arm64.tar.gz.sha256` beside it. The archive holds one folder:

```text
obelisk-cli-<version>-arm64/
  obelisk-cli
  SteamNative/
    obelisk-steam-host
    obelisk-steam-sdk-x86_64
    libobelisk_steam_discovery_adapter.dylib
  Obelisk Rosetta.app/
  LICENSE
```

Keep the folder whole: `obelisk-cli` finds the helpers in `SteamNative` beside itself, and the supervisor finds `Obelisk Rosetta.app` beside `SteamNative`.

Every program is signed with the identity `CLI_SIGN_IDENTITY` names, else the first `Developer ID Application` identity of the Keychain, else the first `Apple Development` one:

| File | Identifier | Hardened runtime | Entitlements |
| --- | --- | --- | --- |
| `obelisk-cli` | `cc.orbiters.obelisk-cli` | yes | none |
| `SteamNative/obelisk-steam-host` | `cc.orbiters.obelisk-cli.steam-host` | yes | `packaging/steam-host.entitlements` |
| `SteamNative/obelisk-steam-sdk-x86_64` | `cc.orbiters.obelisk-cli.steam-sdk-x86_64` | yes | `packaging/steam-host.entitlements` |
| `SteamNative/libobelisk_steam_discovery_adapter.dylib` | `cc.orbiters.obelisk-cli.steam-discovery` | no | none |
| `Obelisk Rosetta.app` | `cc.orbiters.obelisk-cli.rosetta` | yes | none |

A Developer ID signature takes Apple's secure timestamp; an Apple Development one takes none. When the Keychain does not let `codesign` use the key within a minute, or no identity is there, the script says so on standard error and the archive holds the build unsigned. Keep the same identity across builds: the Keychain item that holds the Steam session belongs to the signature that saved it.

With `CLI_NOTARY_PROFILE` naming a profile of `xcrun notarytool store-credentials`, and a Developer ID signature, the script sends the programs to Apple's notary before it archives them, and waits 20 minutes for the answer. No ticket can be stapled to bare programs: macOS asks Apple for it at their first check. Without it the archive is not notarized, and a program that downloads it should remove the quarantine attribute from the folder, or run it from a process that set none.

```sh
cargo test -p obelisk-cli
cargo clippy -p obelisk-cli --all-targets -- -D warnings
```

The tests reach neither Steam nor the Keychain. `cargo test -p obelisk-cli -- --ignored` also saves and forgets one item of the login Keychain, under this program's own service.

The engine files of [Keyboard layout](#keyboard-layout) and [Graphics](#graphics) are checked in, in `crates/game-runtime/engine-patch`, and built by `scripts/build-engine-patch.sh` from an installed copy of the Voile release they are for (as Voile ships it: not one this program already patched) and checkouts of the forks: voile-lkd for `wsd` and `fexdarwin-run` (with `FEX`, a checkout of Zagrios/FEX at the release's commit with its submodules, cmake and ninja; FEX is configured with Homebrew's prefix ignored, since its configure takes fmt and others from Homebrew where it finds them and the release was built without), voile-wine with zig 0.16.0 for the driver (a `wsd` built without it must keep the `wsd_protocol.h` of the voile-lkd commit the checked-in driver goes with), neptune with meson, ninja and pkg-config for the render servers. The manifest it writes lists for each file every SHA-256 an earlier committed manifest or `cli-v*` tag had at that path (`previous`), and it writes none that leaves one out: a new build always replaces the files of every earlier one. A part whose checkout is not given keeps the file checked in, once its SHA-256 is the manifest's. The script refuses sources that are not that release's commits plus ours, a toolchain that does not rebuild the release's own driver byte for byte of code, and one that does not rebuild its render servers byte for byte but for the linker's UUID and the ad hoc signature (the UUID comes from the debug map before the strip, which names the folders Voile's CI built in). Each run gives the render servers a new UUID, hence new SHA-256s. Run it again for each Voile release to follow.

`obelisk-cli` version is the `version` of `crates/obelisk-cli/Cargo.toml`; its releases are tagged `cli-v<version>` on the `cli` branch. It is licensed under the GNU General Public License v3.0, as Obelisk is.
