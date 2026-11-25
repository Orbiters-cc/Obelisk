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
- [The game's output](#the-games-output)
- [Graphics](#graphics)
- [The engine](#the-engine)
- [What the engine does for VRChat alone](#what-the-engine-does-for-vrchat-alone)
- [Questions answered in the player's place](#questions-answered-in-the-players-place)
- [What the app does and this program does not](#what-the-app-does-and-this-program-does-not)
- [Building and signing](#building-and-signing)
- [The timeline generation (developers only)](#the-timeline-generation-developers-only)

## State of this build

0.1.14 changes one engine file, `fexdarwin-run` (Orbiters-cc/voile-lkd tag `orbiters-cli-0.1.14`, `73fe820`, SHA-256 `c23bb43f47a4f763f4376761bf1184a86b2f3e88414038ebc814c9c2ad5fd182`); the four others are 0.1.8's to 0.1.13's, byte for byte, and an ordinary start gives the game the same process, arguments, environment, descriptors and signals as 0.1.13 did. On the engine of 0.1.7 to 0.1.13 a thread that was paused and resumed, as the game's garbage collector does to every thread, could run one instruction twice or lose part of its processor state; under heavy loading on many threads this crashed the game (seen on 2026-11-24: one crash 22 s after entering a crowded world, and 2 crashes in 8 starts with empty caches). The engine now pauses a thread only at exact instruction boundaries, and reaches one within microseconds ([The engine](#the-engine)). No scheduling attribute of any guest thread is changed. Ran on 2026-11-25 with VRChat and this very file (in place of 0.1.13's for each start, by a developer build of 0.1.13): 10 cold starts (no crash, no hang), 25 quick starts and quits (no hang), and 5 minutes in a public world with 18 players and 26 avatars loaded from scratch (no crash); no anti-cheat line in any of the 36. An earlier build of the same fix: 13 cold starts, no crash, one hang at quit that was not attributed. Known and unchanged: the rare freeze of the whole process from a late ptrace attach stop is not addressed, and voile-lkd `bf0af9b` and `248bf43` are still held back. A runtime with 0.1.13's files (`ours-earlier`) gets this one's at its next start, in a transaction. No game was started with this build of the program before its release.

0.1.13 changes no engine file: they are 0.1.8's to 0.1.12's, byte for byte, and an ordinary start gives the game the same process, arguments, environment, descriptors and signals as 0.1.12 did. It adds `steam play --no-saved-arguments` ([Commands](#commands)): a launch whose only player's arguments are the ones it was given. No game was started with this build before its release.

0.1.12 changes no engine file: they are 0.1.8's to 0.1.11's, byte for byte, and an ordinary start gives the game the same process, arguments, environment, descriptors and signals as 0.1.11 did. A lock file that is there is opened to be read and only a missing one is made, so one that cannot be written is held like any other ([Keyboard layout](#keyboard-layout)); a release that lost its own `bin/wine`, and a program its user cannot run whatever others can, are named by [`engine_files`](#error-codes) with the release (0.1.11 ran an older release in place of the first, or installed it again unasked, and started on the second); an install of a release that is not there yet removes an unfinished one's folder only once nothing is seen running from one; a listing of `lsof` that ends in the middle of a record is not whole. No game was started with this build before its release.

0.1.11 changes no engine file: they are 0.1.10's, 0.1.9's and 0.1.8's, byte for byte, and an ordinary start gives the game the same process, arguments, environment, descriptors and signals as 0.1.10 did. It closes what 0.1.10's checks left open: one lock for the whole library holds every launch, Rosetta's too, and every change of its runtimes apart ([Keyboard layout](#keyboard-layout)); the look at what runs tells "nothing runs" from "it cannot be told", and only the first lets a file change; a release is laid only once every member of it is what its archive holds, byte for byte, before its place and again in it ([`engine repair`](#engine-repair)); every file the game's own graphics backend starts from is looked at before a start, and says [`engine_files`](#error-codes); a disk that cannot be repaired is not installed on either. No game was started with this build before its release.

0.1.10 changes no engine file: they are 0.1.9's and 0.1.8's, byte for byte. It proves what 0.1.9 assumed: nothing of the engine's runtimes is replaced, put back or removed while anything of the library's engine runs, by the library's session records and the processes themselves, whatever the locks say ([Keyboard layout](#keyboard-layout)); a repair is one atomic exchange of a release checked whole, or none, and what it replaced goes only once the new one is checked in its place ([`engine repair`](#engine-repair)); recovery deletes nothing that is not behind a verified published set; whatever notices that a runtime lost a file says [`engine_files`](#error-codes). The game's standard error is a blocking pipe again, as in 0.1.8, that is always read, and nothing of the log refuses or disturbs a start ([The game's output](#the-games-output)). `steam play --steam-id` starts a game for one account or not at all. No game was started with this build before its release.

0.1.9 changes no engine file: they are 0.1.8's, byte for byte (voile-lkd `orbiters-cli-0.1.8`, `3a299f2`). A game starts on one complete released generation of them or not at all, held by one lock from its preparation to its exit, which an install and a repair wait for too; the failure is a code of its own, `engine_files`, with the paths and what ends it, and `engine repair` ends it ([Keyboard layout](#keyboard-layout)). What a guest on the Linux engine writes reaches the game's log through the session's supervisor, over a pipe that neither stops nor ends the game whatever becomes of its reader or of the disk, bounded and counted ([The engine](#the-engine)). No game was started with this build before its release: everything above is covered by tests with disposable processes, and by the upgrade from the released assets of 0.1.2 to 0.1.8.

0.1.8 puts the engine files in as a recorded transaction and pins a running game's files, bounds a log flood, and records when a Quit was accepted. Its engine files come from Orbiters-cc/voile-lkd branch `orbiters-cli-0.1.8` (`3a299f2`): `fexdarwin-run` from the same sources as 0.1.7's (the hang fix `9bf6dfa`, unchanged), and `wsd` with the bundle's own Dock icon back when no client with an icon remains. **Held back:** voile-lkd `bf0af9b` (the attach stop as the trace link's own state, with the event ring) was built into a candidate of this release and soaked: in one VRChat start of five, VRChat's Easy Anti-Cheat reported "Forbidden system configuration (Debugger detected.)" 26 s in and the game was refused its instance; that line is in none of the 26 earlier VRChat logs on the Mac tested. Its cause is not known (the new code's attach windows measured as short as before), and the anti-cheat comes first: it is not shipped until that is understood ([The engine](#the-engine)). Ran on 2026-11-23 with this build's files: two VRChat starts (08:39 and 08:43, private instances, the screen locked), both arrived in 44 and 38 s and quit by session after about 190 s, no group stop, no anti-cheat line; the first one's launch replaced the candidate's `fexdarwin-run` (`ours-earlier`) in a transaction; the log was whole (69 and 70 presents lines, no flood to count). The candidate's first start had shown the bounded writer printing to the launcher's discarded standard output (no guest line in the log), fixed before this build. The engine files change only in a transaction whose record (id, phase, every file's SHA-256 before and after) is written before each step, and only while no game runs on the runtime: after a crash, a full disk or a lock that cannot be taken a launch starts on one complete set or is refused (`wine_incomplete`) with a line on standard error naming the path and what to do; the release is checked before anything is recovered ([Keyboard layout](#keyboard-layout)). The Linux engine's launch script writes the guest's output through a bounded writer: a line repeated without end is kept 200 times, then counted, and every line a frame counter reads passes. A Game session record says `quit_accepted`, when its supervisor accepted a Quit. Bridges and other programs that need the whole drawing size can pass `steam play --render-percent 100`: it overrides an installation's saved percentage for that launch.

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
| The locks of the runtimes (from 0.1.9), never to be removed | `<library>/Voile.locks/`: from 0.1.11 `library.lock`; `<release>.writer.lock`, `<release>.sessions.lock`, `install.lock`. From 0.1.10 the folder comes with the library: every command that uses the library makes it where it is missing and the library can be written |
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
| `log_failure` | string or absent | From 0.1.10, while a game runs: why its log does not hold what it wrote (the log could not be opened, the output's pipe could not be made). A sentence for a log. A game never waits for, or is refused over, its log. |
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

### `steam play <app_id> [--steam-id <id>] [--render-percent <1..100>] [--no-saved-arguments] [--no-free-pointer] [--no-shared-textures] [-- <argument>...]`

Starts the installed Windows build as the app's Play does and stays attached until its Game session is finished:

1. [`preparing`](#preparing).
2. [`progress`](#progress), while a Transfer repairs an interrupted installation or installs Steam's newer build, and while a runtime that is missing or outdated downloads. [`voile_update`](#voile_update) when a newer Voile release is taken.
3. [`starting`](#starting), once the game's supervisor holds it, with its Game session.
4. [`running`](#running), with the game's process ID.
5. [`cloud_pending`](#cloud_pending), only when Steam Cloud asks a question after the game exited.
6. [`exited`](#exited), with the game's exit code.

`--steam-id` (from 0.1.10) names the Steam account the game may start for: with another one signed in it fails with `other_account` before anything is started or reached. The saved session is read once, at the start of the command, and the one that was checked is the one the game starts with: an account changed meanwhile never starts in its place.

`--render-percent` (from 0.1.7) has a game on Voile's Linux engine drawn at that share, per side, of the display's drawing resolution (100: as Voile draws it, the display's backing pixels): the engine's window server tells the game a smaller monitor and macOS scales the picture to the screen. For this launch only, over the game's own launch settings (`render_percent`), never kept. It must come before `--`; a value that is not a whole number from 1 to 100 is a `usage` failure. A game on Rosetta's Wine, whose engine offers none, ignores it. The window server writes what it applied in the game's log (`wsd: render percent: requested <n>, applied <n>; ... (desktop <w>x<h> px)`).

`--no-saved-arguments` (from 0.1.13) starts the game without the arguments kept in its launch settings (the app's Launch Settings arguments): those after `--` are then the only ones of the player's, and none of them means none. For this launch only: what is kept stays kept, and every other launch setting (engine, graphics, render percent, Game Mode, the Performance HUD) applies as ever; so do Steam's own arguments and the game profile's. It is for a program that must know exactly what it starts the game with, such as one that names a VRChat profile: a kept `--profile=1` would reach the game beside, and before, the one it names.

`--no-free-pointer` and `--no-shared-textures` (unreleased) start VRChat without what the engine does [for it alone](#what-the-engine-does-for-vrchat-alone), for this launch only. No option turns either on for another game.

All five come before `--`, in any order.

The arguments after `--` start the game after Steam's own, the game profile's and those kept in its launch settings (unless `--no-saved-arguments`), where the app's Launch Settings arguments go. Each is passed as it is given, never split or joined, and none is kept: the next launch does not have them. They are written to the head of the game's log, so pass no secret there.

From `starting` on, the supervisor (`obelisk-steam-host`, in its own session) owns the game: the game runs on when `obelisk-cli` is killed, and `steam running` and `steam quit` find it. Before `starting`, the launch lives in this process: killing it abandons the launch, and a Transfer it interrupted resumes at the next `steam play` or `steam install`.

It fails with `not_signed_in`, `other_account`, `helpers_missing`, `not_installed`, `already_running` when a game of that app runs, `another_game_running` when another Steam game does, or one of the launch failures [below](#error-codes).

### `steam running`

Tells one [`session`](#session) event for the Game session that holds Steam in this library, if there is one, then [`done`](#done). It needs `obelisk-steam-host`, which tells whether the recorded processes still live.

### `steam quit <app_id> [--session <id>] [--force] [--wait <seconds>]`

Asks the running game to end, as the app's Quit does: it is asked to close its windows, and nothing is killed. A launch still preparing under its supervisor is cancelled. `--force` is the app's Force Quit: the game ends now, with whatever still runs for its installation, and its session still settles its Cloud saves.

It tells [`quit_requested`](#quit_requested) once the supervisor took the request. With `--wait`, it then waits that many seconds for the Game session to end and tells [`exited`](#exited), or fails with `still_running`. A game may answer Quit with a question of its own and play on.

With `--session`, only the game of that Game session (the `session_id` of its [`starting`](#starting) or [`running`](#running) event) is asked to end: when another session of the app runs, it fails with `other_session` and nothing is asked.

It fails with `not_running` when no game of that app runs.

### `engine repair`

From 0.1.9. Installs the Voile release Windows games run in again, over the one in place, whatever is missing or foreign in it or an engine update left cut short there: the repair of an [`engine_files`](#error-codes) failure. It tells [`engine_repairing`](#engine_repairing) events, then [`engine_repaired`](#engine_repaired) with this build's engine files put in and the set proven. The release in place, with every file an update kept, stays until the new one is downloaded, its archive's digest checked, and what was laid of it found to be what the archive holds: from 0.1.11 every member's type, size, mode (its owner's bits), link and SHA-256, as a listing of that very archive tells them (`tar --format=mtree`, read beside the extraction), the Linux engine's files Voile's own, and whatever a game of any backend starts from there (0.1.10 looked at the members' names only). It is then exchanged for it in one step (`RENAME_SWAP`), under the lock every launch holds, and goes only once the new one is found whole again, by the same comparison, in its place (it takes its place back otherwise). An install of a release that is not there yet is held to the same, and from 0.1.11 is refused like a repair on a disk that could not be repaired. Killed at any step, the old release or the new one is whole in its place; what an interrupted repair saved is kept until a repair of that same release succeeds (from 0.1.11 one of another release never removes it; from 0.1.12 nothing an unfinished install left is removed while something is seen running from the runtimes). `busy` while anything of the library's engine runs, or when that cannot be told. It fails with `voile_failed` without the network, with a development runtime in place, or (key `backend.voile.noExchange`, with the path) on a disk that cannot exchange two folders in one step, as an APFS disk does: there is no other way to repair in place. It needs no signed-in account. Nothing is to be deleted by hand.

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
| `session_id` | string or null | From 0.1.10: the Game session this very launch was admitted as, before its game runs. |

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

### `engine_repairing`

Told at most every 250 ms while [`engine repair`](#engine-repair) downloads and lays the release.

| Field | Type | Meaning |
| --- | --- | --- |
| `bytes_done` | number | Bytes of the release downloaded. |
| `bytes_total` | number | Bytes of the release in all. |
| `fraction` | number or null | From 0 to 1. |

### `engine_repaired`

| Field | Type | Meaning |
| --- | --- | --- |
| `voile` | string or null | The Voile release laid again. |
| `keyboard_layout` | string | As in [`status`](#status-1): `mac` when this build's engine files are in place. |
| `engine_files` | array | As in [`status`](#status-1): each engine file, proven with the others to be one complete set. |

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
| `engine` | object | From 0.1.9, under the code `engine_files`; absent otherwise (in 0.1.9 also under `launch_failed` with the key `backend.runtime.wineIncomplete`): `paths` (array of strings: the files that are not what they must be, by their path in the Linux engine's runtime; empty when they are not known), `reason` (string: a sentence in English for a log), `action` (string, what ends it: `repair`, run [`engine repair`](#engine-repair); `quit_game`, quit the game that runs on them, then start again; `check_storage`, the runtime's lock cannot be held: the library's folder must be writable and on a disk that takes locks). |

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
| `other_account` | From 0.1.10: `steam play --steam-id`: another account is signed in. Nothing was started. |
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
| `busy` | Another operation holds the game: a game runs (for `engine repair`, anything of the library's engine, or it cannot be told), a Transfer of the game runs, or another process installs it. |
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
| `engine_files` | From 0.1.9: no game starts on the Linux engine's files, which are not one complete released set or cannot be held as they are. `engine` names them and what ends it. Nothing was changed, and every file an engine update kept is still there. From 0.1.10 also whenever a runtime lost a file a launch needs, whichever check noticed it first: the engine's program or its Wine (named in `paths`; from 0.1.11 so is every file the game's own graphics backend and engine start from, `graphics/bin/virgl_test_server` among them, that is gone, no file, cut short or not to be run; from 0.1.12 not to be run by this very user, and the release's own `bin/wine` for a game of either engine: a release is the one in place whatever it lost, and none replaced by it is removed while it has no Wine), a library (key `backend.runtime.wineIncomplete`), the runtime or Rosetta's Wine (key `backend.runtime.wineUnavailable`). See [Keyboard layout](#keyboard-layout). |
| `voile_failed` | A Voile release could not be downloaded, read or stored, or (key `backend.voile.noExchange`) its disk cannot be repaired in place. |
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

From 0.1.9 a game starts on one **complete released generation** of the files or not at all: the exact SHA-256 of every file, either all as Voile ships them for the release the runtime declares, or all as one published build of this program left them (the manifest's `generations`, read by `scripts/engine_patch_manifest.py` from every `cli-v*` release; an unreleased build's file is replaced, and is no set to start on). Every path that lets a launch go on ends with that proof, under the runtime's lock: nothing to change, a switch made or undone, a full disk at any write, an unwritable runtime, another game holding the files. Anything else (a mixture, a foreign, missing or unreadable file, a link in a file's place, files of ours in another release) fails with [`engine_files`](#error-codes), which names the paths and what ends it. Until 0.1.8 a mixture could start a game when it could not be changed at that moment, and a foreign or missing file left the runtime as it was and started a game. A runtime of another release, or a development one, is not this program's to prove: its games run as Voile ships it, pinned all the same.

The files change in a recorded transaction (from 0.1.8) and never under a running game. From 0.1.11 one lock holds the whole library, `<library>/Voile.locks/library.lock`, beside `Voile` and outside every folder an install replaces. Every launch of a Windows game, on either engine, holds it shared from before its runtime is picked to its game's exit; whatever changes a runtime (a launch's engine update, its recovery and cleanup, `engine repair`, an install, the removal of replaced releases) holds it alone from before the look below to its last check, and without it changes nothing (`busy`; a launch then starts when the files are one complete set as they are). A launch waits for a change under way. The hold is one open lock for the whole launch: the preparation takes it, the launch's own request carries it (whatever ends the launch, a failure, a cancellation or its start, lets go of it, for every storefront), and the supervisor is started with the same open file, as its standard output, and keeps it until it ends: there is no moment without it, whichever process dies. From 0.1.12 a lock file that is there is opened only to be read (macOS grants either lock on such a descriptor), and one is made only where it is missing: a file nobody may write is held like any other, and what is no regular file is no lock. A preparation that cannot hold it fails with `engine_files` (`check_storage`); a supervisor of a Windows game that was not given it accepts no launch.

The two locks of one release, `<release>.writer.lock` (whoever looks at or changes the files) and `<release>.sessions.lock` (shared by every launch on the Linux engine, from its preparation to its game's exit), were 0.1.9's and 0.1.10's only ones, and are still held beside it: a process of those builds, left running across an upgrade, knows no other. So are, where they are there, the locks an obelisk-cli before 0.1.9 kept in the runtime, `linux/.obelisk-engine-*.lock` (from 0.1.10 one that is there and cannot be opened is an error, never "absent", and a launch pinned on files an older build would change holds the older build's lock too).

The locks are the fast path and not the proof (from 0.1.10). Before a file of a runtime is replaced, put back or removed, under the lock held alone, two looks must both show that nothing of the library's engine runs: the Game session records of every storefront in the library (their supervisor, their game and the helper that becomes it, by process ID, user and birth, as `ps` tells them), and every process that runs or maps a file from under `<library>/Voile` (`fexdarwin-run`, the render servers, Wine on Rosetta), as `lsof` tells it. Both only read what the system tells of processes. The window server runs from a copy of its own, in the temporary folder, under a launcher the records name; one left alone by both uses nothing of the runtimes. The answer is one of three (from 0.1.11): nothing runs, something does, or it cannot be told, and only the first lets a file change. It cannot be told when a listing could not be run, was killed, took longer than 10 s, ended with a status that is not success, wrote anything on its standard error, or wrote what does not read to its end (from 0.1.12 a record cut at the end of a line too), when `lsof`'s leaves this very process out, or when a session is named whose record is missing or cannot be read (`ps`, asked for processes none of which is there, ends with 1 and not a word: that is an answer). So a game whose supervisor died and a game an older build started keep their files. A launch that finds something running, or cannot tell, changes nothing and starts when the files are one complete set as they are, a pending cleanup included (it waits); else it fails with `engine_files` (`quit_game` for an update cut short). A launch on files that need nothing looks at no process.

Accepted limits. A start by an obelisk-cli 0.1.10 or older does not know the library's lock: what it races is covered by the look at what runs and by the one release's locks it knows, and no further (a game of 0.1.10 on Rosetta's Wine, between its preparation and its supervisor's record, is seen by neither). An unrelated program that maps a file under the library makes a change refuse, naming its process and the file. A supervisor suspended by a debugger or `SIGSTOP` stops reading the game's standard error, as 0.1.8's reader did, and the game waits once its pipe is full. A disk that stands still holds whoever waits on it.

The transaction writes its record, `linux/.obelisk-engine-patch.txn.json` (an id, the phase `staging`, `switching` or `committed`, the Voile release, and each file's exact SHA-256 before and after), synced, before each phase; ours are written beside the files as `.obelisk-txn-<id>-new-<name>`, each original linked aside as `.obelisk-txn-<id>-old-<name>` (named by the transaction and the destination), their folders synced, all switched, every file checked, then `committed`. A step whose folder cannot be synced did not happen: it is undone, or the launch refused with the record kept. At the next launch a record left by a crash is ended from it alone, once the record itself is one this program can have written (its id; every path of the set exactly once and no other; each `before` a file that path may have had, each `after` one of ours and from 0.1.10 all of them together one published set) and everything its end needs has been read and checked; nothing is put back before that, and the kept originals stay until every file is back and checked. Committed, only its kept originals are removed (a failure to remove them costs nothing, and the update is never undone); any other phase puts every file back to its `before`. From 0.1.10 the kept originals and the record go only once the files are one published set, verified: a set that is not one (a mixture that update was completing) keeps them, until a later update makes it whole. The release is checked before anything is recovered. A record that is not one, a kept original that is missing, unreadable or not the digest the record names refuses the launch, the record and the kept originals left where they are. Leftovers of older builds and of no transaction go only once the set is proven whole: until then they may be its only good copies. The originals an older build kept aside (`.obelisk-original-<name>`) are never installed over a file that is there; only the sole copy of a file that is gone is put back. Why not a separate runtime folder per generation: the runtime's path is part of the Wine prefix's state and of what the game and its anti-cheat see, and moving every session to a new folder at each update could not be soaked in the few starts a Mac may make; the pin gives a running session the same guarantee (nothing it uses changes) without moving anything.

From 0.1.7 a file that an earlier build of this program put in place counts as one to replace, like the release's own: the manifest lists, for each path, every SHA-256 an earlier build put there (`previous`), and `scripts/build-engine-patch.sh` writes no manifest that leaves one out. A kept original goes back only when it is the release's file or an earlier one of ours (another file under that name is removed, never installed), and temporaries a crash left beside the files are removed. A switch that fails, and whose undo fails too, leaves files of two sets: the launch is refused (from 0.1.9 with `engine_files`) rather than a game started on them; the next launch tries again from what was kept aside.

From 0.1.6 the files go in as one, before a launch on the engine: under a lock of the runtime (`linux/.obelisk-engine-patch.lock`), the files are looked at once, each of ours is written beside the file it replaces and checked by its SHA-256, each original is kept aside (`.obelisk-original-<name>` beside it), then all are switched; a switch that fails part way puts every original back, and one a crash cut short is undone at the next launch before anything else. Older builds switched file by file: a runtime they left with some of each is `pending`, and the next launch puts all of this build's in.

Tested: the layout code against the Mac's own US, French, French - PC, German, Dvorak, ABC - AZERTY and Russian layouts (`tests/keyboard` in voile-lkd, 569 checks); and a Windows program run through the patched engine of a copy of `v26.3.0-27.beta1`, with keys injected by `wsdctl`, under French - PC, French, German and US layouts and across a change of input source: scan codes, virtual keys, raw input, characters, AltGr, dead keys, `MapVirtualKey`, `VkKeyScan` and `GetKeyNameText`. Never run: a key from a real keyboard, and a game.

## The game's output

What a game's guest on the Linux engine writes to its standard error goes to its log through its supervisor, which keeps a repeated line 200 times and then counts it, gives every class of lines a budget, and everything the guest writes a cap (under 192 MiB; `game_log.rs`). What the engine's own programs on the Mac write (`metal-HUD:`, the window server's) goes to the log beside this and is not in that cap.

From 0.1.10 the game writes into a blocking pipe, as it did until 0.1.8 (0.1.9's never blocked, and a write could fail with `EAGAIN`). One thread of the supervisor always reads it into a bounded queue, another writes the queue to the log: a disk that stands still or is full costs lines, which are counted and said. A process of its own (`/bin/sh`, then `/bin/cat`) holds the pipe's reading end beside the supervisor, unread, and drains it to nothing once that thread or the whole supervisor is gone, so that the game meets neither a full pipe nor a broken one; it holds the launch's lock too, as long as anything holds the game's end. The game's end reaches it as the exec helper's standard error, which the helper duplicates as it becomes the game: no descriptor of the supervisor is ever made inheritable.

Nothing of the log refuses a game, or is a reason to write it uncapped: a log's folder, log, header, pipe or thread that cannot be had leaves the game's output going nowhere, and the session's record says why (`log_failure`).

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

**Not in 0.1.8's files (held, see above): the attach stop, done as a link's state.** A verification of 0.1.7 reproduced three more ways the attach's SIGSTOP, still a queued signal, could go wrong: a second tracer's SIGSTOP coalesced with a departed tracer's and dropped with it, an ordinary SIGSTOP coalesced the same way, and a SIGCONT to another thread flushing it. In `bf0af9b` the stop is part of the link (set in the same critical section as the link, cleared only when reported to that generation of it, when a SIGSTOP stop of the same link takes its place as in Linux, or with the link); the tracee is told by a hint the tracer's wake (bound to the link's slot and 64-bit generation) and every flag sync set while a stop is pending, and takes it before any queued signal, deciding from the link itself. The flag sync reads the table and writes the tracee's flags under one lock; an attach whose wake cannot be delivered is undone (ESRCH); a tracer killed between linking and waking leaves its tracee running, and the next attach reclaims the link; a parked tracee of a killed EXITKILL tracer dies after detaching; clone reads the parent's link and makes the child's together; a non-leader exec ends a link made to the leader's tid in its rename window. **The event ring** (an engine with voile-lkd `bf0af9b` or later; 0.1.8 sets the variable, its engine ignores it): LKD writes every attach, attach stop, detach, cancelled link, group stop and SIGCONT resume, in order, to `LKD_EVENT_RING`, which the launcher sets to the game's log's name with the extension `.lkd-events` (for Steam, `~/Library/Logs/Obelisk/Steam/wine-438100.lkd-events`), new at each start: a header line, then 8192 fixed lines of 128 bytes, the newest over the oldest; sort the lines after the first to read them in order. The game's log keeps only a few of these lines. **The log flood:** after a renderer's graphics went for good, Neptune's guest driver retried without end and wrote 420 MB in five minutes; 0.1.8's launch script wrote the guest's output through a bounded writer (`awk`): the guest stopped when the writer stood still, and ended (SIGPIPE) when it died, and distinct lines had no cap. **From 0.1.9** the session's supervisor is that writer (`crates/game-runtime/src/game_log.rs`). The guest's standard error is a pipe whose writing end never blocks: a line that finds it full is lost to the log, and the write returns. The launch script keeps the pipe's reading end open, unread, until the game is gone: a supervisor that is gone first leaves a full pipe, never a broken one. In the supervisor one thread reads the pipe into a queue of at most 8 MiB, another filters it into the log, so that a disk that stands still or is full costs lines only, counted: `[obelisk-log: <n> bytes of the game's output dropped here, ...]`, `[obelisk-log: <n> bytes could not be written here]`, and at the end `[obelisk-log: output lost in all: <n> bytes dropped, <n> bytes not written; the pipe was full <n> times]` when anything was. A line (numbers aside) is kept 200 times, then a count at 1,000, 10,000, 100,000 and every million, and the totals at the end; a line longer than 4 KiB is cut. Lines without a mark take at most 64 MiB of a log; the marked ones pass as they are within a budget of their own each (32 MiB for `neptune unixlib wsi` and `metal-HUD`, 4 MiB for `guest swapchain`, `wsd:`, `lkd:`, `obelisk-wait:` and a lost renderer's two), past which one passes every 10 s, so that a frame counter that goes on is never taken for one that stopped; everything the guest writes together stays under 192 MiB. What the engine's own programs on the Mac write (Metal's `metal-HUD:` lines from the window server, which the session's frames are counted from) goes to the log itself, never through the pipe. The log is never cut or rewritten (the frame counters keep their continuity). An end of the game waits at most 4 s for what is left in the pipe and for the log. The driver itself (inside the game's process) is unchanged.

**Pauses (from 0.1.14).** A guest thread is paused by a host signal (a guest signal, or Wine's `SuspendThread`, which a stop-the-world collector sends to every thread) and starts again at a guest instruction, from its registers and its context alone. Until 0.1.13 it was paused wherever it was in the translated code, which is not always between two guest instructions: the instruction that had run in part ran again, or what it had put aside (the carry, the upper halves of the 256-bit registers, the MMX and x87 registers, the direction flag) was lost; a fault the guest resolves, a guard page or a write watch, had the same defects for some instructions. From 0.1.14 an asynchronous stop happens at a safe point alone: the entry of a translated unit, the head of a loop, or between two parts of a long string instruction, each of which makes an access through a pointer that costs nothing until a stop is asked for; and an instruction that can fault leaves everything where the fault's handler finds it. FEX's changes are patches in voile-lkd (`fexdarwin/fex-patches`), which its build applies to a copy of FEX; they are this engine's alone, not contributions to FEX. The engine sets no priority, policy or QoS of a thread that runs the guest, and imports no function that could (`tests/signal-restart/run.py --group sched` compares every scheduling attribute of every thread with the released engine's). `tests/signal-restart` in voile-lkd holds the tests: every group passes on this file, and nearly all of `correct` fails on 0.1.8's to 0.1.13's. On a busy machine a pause still waits for the thread's turn on a core, as a native thread's signal does. A process that hangs can be looked at with `fexdarwin-run --dump` (voile-lkd's `docs/hang-evidence.md`). Not addressed: the freeze of the whole process from an attach's SIGSTOP that comes late (above; `lkd: ... signal 19 ... stops process` in the game's log).

**The Dock icon.** Each Wine program sends its icon to the window server as it attaches, and the last one stayed: VRChat's `yt-dlp.exe`, started by a world's video player, put its own in the Dock. The window server now shows the icon of the client whose window first got a swapchain's plane (the game) until it disconnects, before that the one whose window was shown first, and never one without a window over one with a window (`tests/graphics/run-wsd-icon.sh`, two synthetic clients; the window server logs `application icon shown: ...`).

Tested: the release's own `fexdarwin-run` rebuilt from its sources byte for byte but for the linker's UUID and the ad hoc signature (`scripts/build-engine-patch.sh` refuses to write a patch otherwise); LKD's ptrace and signal tests on ours; a guest shell run through it with a job-control stop and continue; the files of every earlier build (`cli-v0.1.2` to `cli-v0.1.6`) over a copy of the release as Voile ships it are replaced by these (`every_earlier_release_s_files_are_replaced`, run with `OBELISK_PRISTINE_RELEASE`).

## What the engine does for VRChat alone

Unreleased. For VRChat (Steam app 438100) on the Linux engine, and for no other game, two of the engine's programs on the Mac's side do something more; the launcher (`launch.sh`) gives them the options, and the game's log names them in its head (`free_pointer=on shared_textures=on`). Nothing the game runs, loads or maps is another for it, and any other game's launch is, argument for argument and variable for variable, what it was. It is no field of the game profiles, which Obelisk reads again from `main`.

| | `--free-pointer on` (`steam play --no-free-pointer` leaves it out) | `--shared-textures on` (`--no-shared-textures`; DXMT only) |
| --- | --- | --- |
| program | the window server, `wsd --free-unconfined-pointer --pointer-socket <host folder>/pointer.sock` | the render server, `NPT_SHARED_PUBLISH=<host folder>/shared` |
| what | the Mac's pointer is held only while the game confines it: free while VRChat's menus are open, as on Windows | a texture VRChat shares (its stream camera's, for Spout) gets a share handle, and its frames can be read by a program of the same user |
| for local programs | the pointer's state, the game window's place, and the raw moves while it is held (voile-lkd's `wsd/PROTOCOL.md`, "Pointer channel"; `wsd/wsd_pointer.h`) | each live shared texture: token, size, format, whole frames in memory mapped read-only (Neptune's `virglrenderer/docs/shared-textures.md`; `npt_publish_wire.h`) |
| in the log | `wsd: ... pointer held by the game` / `pointer freed by the game` | `npt: publish: ...` |

The **host folder** is `<TMPDIR>/obelisk-engine-<pid>.host`, mode 700, where `<pid>` is the [Game session record](#game-session-records)'s `game.pid` and `<TMPDIR>` the launch's (the user's temporary folder, `getconf DARWIN_USER_TEMP_DIR`). It is beside the session's sockets (`obelisk-engine-<pid>`, which the game sees as `/run/obelisk`) and not in them: the game does not see it, and Force Quit, which ends whatever uses a file of the sockets' folder, never takes a program that reads here for the game's. It goes with the session; a launch that cannot make it starts without either channel. Both channels only tell: a reader that sends anything is disconnected, one that does not read is dropped, and nothing waits for a reader.

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

The engine files of [Keyboard layout](#keyboard-layout) and [Graphics](#graphics) are checked in, in `crates/game-runtime/engine-patch`, and built by `scripts/build-engine-patch.sh` from an installed copy of the Voile release they are for (as Voile ships it: not one this program already patched) and checkouts of the forks: voile-lkd for `wsd` and `fexdarwin-run` (with `FEX`, a checkout of Zagrios/FEX at the release's commit with its submodules, cmake and ninja; FEX is configured with Homebrew's prefix ignored, since its configure takes fmt and others from Homebrew where it finds them and the release was built without; from 0.1.14 voile-lkd's build applies its `fexdarwin/fex-patches` to a copy of it), voile-wine with zig 0.16.0 for the driver (a `wsd` built without it must keep the `wsd_protocol.h` of the voile-lkd commit the checked-in driver goes with), neptune with meson, ninja and pkg-config for the render servers. The manifest it writes lists for each file every SHA-256 an earlier committed manifest or `cli-v*` tag had at that path (`previous`), and it writes none that leaves one out: a new build always replaces the files of every earlier one. A part whose checkout is not given keeps the file checked in, once its SHA-256 is the manifest's. The script refuses sources that are not that release's commits plus ours, a toolchain that does not rebuild the release's own driver byte for byte of code, and one that does not rebuild its render servers byte for byte but for the linker's UUID and the ad hoc signature (the UUID comes from the debug map before the strip, which names the folders Voile's CI built in). Each run gives the render servers a new UUID, hence new SHA-256s. Run it again for each Voile release to follow.

`obelisk-cli` version is the `version` of `crates/obelisk-cli/Cargo.toml`; its releases are tagged `cli-v<version>` on the `cli` branch. It is licensed under the GNU General Public License v3.0, as Obelisk is.

## The timeline generation (developers only)

Nothing of this section is in a release, and this branch (`host-features-dev`) is none's: only a build with the Cargo feature `developer-timeline` has it, and Orbiters - N4N0 never uses it. "timeline" is the name the other generation has in these programs (it was first made for measurements, branch `perf-timeline`); here it is the released set (0.1.14) with the host programs of `host-features` in place of three files: `engine/wsd` and the two render servers. Everything the emulated game runs, loads or maps is the released set's, byte for byte: `scripts/host_generation.py` compares every entry of the two trees and writes nothing when anything else differs. The digests are built in (`engine-patch/timeline.json`); the files come from the folder the script wrote.

| Variable | |
| --- | --- |
| `OBELISK_DEVELOPER_ENGINE` | `timeline`: this process puts that generation in place and starts games on it, and on nothing else. `released`, or unset: the released set, as every build does; files of the other generation that are in place are replaced before any game starts, and a game does not start on them when they cannot be. Anything else is a `usage` error. |
| `OBELISK_DEVELOPER_ENGINE_FILES` | The generation's folder, an absolute path. Needed by `timeline`. |
| `OBELISK_REHEARSAL_HOST` | `engine rehearse` alone: what of [What the engine does for VRChat alone](#what-the-engine-does-for-vrchat-alone) the rehearsal has: `pointer,textures` (unset), one of them, or `none`. |

A start with one of the two left out: `steam play 438100 --no-free-pointer` or `--no-shared-textures`.

`engine generation` puts the generation this process was asked for in place, as a launch does and by the same code (one recorded transaction under the library's locks, refused while anything of the engine runs), proves it, and tells one `engine_generation` line: `generation`, `voile`, and `engine_files` (each `path` with its `sha256`). `engine rehearse <folder> <program> <1..100> <log> [-- <argument>...]` runs an owned Windows program of that folder as a game starts on the engine, without Steam, and tells `rehearsal_running` (`pid`) and `rehearsal_exited` (`exit_code`).

```sh
OBELISK_DEVELOPER_ENGINE=timeline OBELISK_DEVELOPER_ENGINE_FILES=<folder> obelisk-cli engine generation   # in
OBELISK_DEVELOPER_ENGINE=released OBELISK_DEVELOPER_ENGINE_FILES=<folder> obelisk-cli engine generation   # back out
```

A released obelisk-cli that finds the timeline generation in a library takes its files for foreign ones: it starts no game (`engine_files`, `repair`) until the generation is put back as above, or `engine repair` lays the release again.
