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
- [Questions answered in the player's place](#questions-answered-in-the-players-place)
- [What the app does and this program does not](#what-the-app-does-and-this-program-does-not)
- [Building and signing](#building-and-signing)

## State of this build

0.1.2 gives Windows games on Voile's Linux engine the keyboard layout selected on the Mac, for Voile `v26.3.0-27.beta1`: see [Keyboard layout](#keyboard-layout), which also tells what was tested. `status` tells it in `voile.keyboard_layout`. 0.1.1 is 0.1.0 signed with a Developer ID and sent to Apple's notary. It is a first build. `status`, `steam installed`, `settings prereleases`, the command line, every failure before Steam is reached, and `steam login --qr` up to its `challenge` event were run for real. A complete sign-in, `steam install`, `steam play`, `steam running` with a live session and `steam quit` were never run against Steam: their code follows the app's and is covered by tests that drive it with the engine's states, not by a launch.

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

### `steam play <app_id> [-- <argument>...]`

Starts the installed Windows build as the app's Play does and stays attached until its Game session is finished:

1. [`preparing`](#preparing).
2. [`progress`](#progress), while a Transfer repairs an interrupted installation or installs Steam's newer build, and while a runtime that is missing or outdated downloads. [`voile_update`](#voile_update) when a newer Voile release is taken.
3. [`starting`](#starting), once the game's supervisor holds it.
4. [`running`](#running), with the game's process ID.
5. [`cloud_pending`](#cloud_pending), only when Steam Cloud asks a question after the game exited.
6. [`exited`](#exited), with the game's exit code.

The arguments after `--` start the game after Steam's own, the game profile's and those kept in its launch settings, where the app's Launch Settings arguments go. Each is passed as it is given, never split or joined, and none is kept: the next launch does not have them. They are written to the head of the game's log, so pass no secret there.

From `starting` on, the supervisor (`obelisk-steam-host`, in its own session) owns the game: the game runs on when `obelisk-cli` is killed, and `steam running` and `steam quit` find it. Before `starting`, the launch lives in this process: killing it abandons the launch, and a Transfer it interrupted resumes at the next `steam play` or `steam install`.

It fails with `not_signed_in`, `helpers_missing`, `not_installed`, `already_running` when a game of that app runs, `another_game_running` when another Steam game does, or one of the launch failures [below](#error-codes).

### `steam running`

Tells one [`session`](#session) event for the Game session that holds Steam in this library, if there is one, then [`done`](#done). It needs `obelisk-steam-host`, which tells whether the recorded processes still live.

### `steam quit <app_id> [--force] [--wait <seconds>]`

Asks the running game to end, as the app's Quit does: it is asked to close its windows, and nothing is killed. A launch still preparing under its supervisor is cancelled. `--force` is the app's Force Quit: the game ends now, with whatever still runs for its installation, and its session still settles its Cloud saves.

It tells [`quit_requested`](#quit_requested) once the supervisor took the request. With `--wait`, it then waits that many seconds for the Game session to end and tells [`exited`](#exited), or fails with `still_running`. A game may answer Quit with a question of its own and play on.

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
| `voile` | object | `installed` (string or null): the Voile release Windows games run in now, such as `v26.3.0-16`. `prereleases` (boolean): pre-releases are taken. `keyboard_layout` (string or null): the keyboard the games of the Linux engine get, see [Keyboard layout](#keyboard-layout): `mac` (the layout selected on the Mac), `pending` (the same, from the next launch on the engine), `us` (a US keyboard whatever the Mac's layout: this build has no engine files for the release in place). Null when no release with the Linux engine is installed. |
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

### `exited`

| Field | Type | Meaning |
| --- | --- | --- |
| `app_id` | number | The game. |
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

Tested: the layout code against the Mac's own US, French, French - PC, German, Dvorak, ABC - AZERTY and Russian layouts (`tests/keyboard` in voile-lkd, 569 checks); and a Windows program run through the patched engine of a copy of `v26.3.0-27.beta1`, with keys injected by `wsdctl`, under French - PC, French, German and US layouts and across a change of input source: scan codes, virtual keys, raw input, characters, AltGr, dead keys, `MapVirtualKey`, `VkKeyScan` and `GetKeyNameText`. Never run: a key from a real keyboard, and a game.

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

The engine files of [Keyboard layout](#keyboard-layout) are checked in, in `crates/game-runtime/engine-patch`, and built by `scripts/build-engine-patch.sh` from checkouts of the two forks, zig 0.16.0 and an installed copy of the Voile release they are for. The script refuses sources that are not that release's commits plus ours, and a toolchain that does not rebuild the release's own driver byte for byte of code. Run it again for each Voile release to follow.

`obelisk-cli` version is the `version` of `crates/obelisk-cli/Cargo.toml`; its releases are tagged `cli-v<version>` on the `cli` branch. It is licensed under the GNU General Public License v3.0, as Obelisk is.
