#!/bin/bash
# Builds obelisk-cli and the Steam helpers its launches run, in release for Apple silicon, puts them in one
# folder, signs each, and writes build/dist/obelisk-cli-<version>-arm64.tar.gz with its .sha256 beside it.
#
#   obelisk-cli-<version>-arm64/
#     obelisk-cli
#     SteamNative/obelisk-steam-host, obelisk-steam-sdk-x86_64, libobelisk_steam_discovery_adapter.dylib
#     Obelisk Rosetta.app      the Intel helper that tells whether Rosetta is installed
#     LICENSE
#
# The helpers are those scripts/build-steam-native.sh puts in Obelisk.app, built the same way without Xcode's
# environment. CLI_SIGN_IDENTITY names the signing identity; without it, the first "Developer ID Application"
# identity of the Keychain, else the first "Apple Development" one. A Keychain that does not answer for the key
# within a minute leaves the build unsigned, and the script says so.
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if [[ "$(uname -m)" != arm64 ]]; then
    echo "obelisk-cli is built for Apple silicon." >&2
    exit 1
fi
for TARGET in aarch64-apple-darwin x86_64-apple-darwin; do
    if ! rustup target list --installed | grep -qx "$TARGET"; then
        echo "The Rust target $TARGET is missing: rustup target add $TARGET" >&2
        exit 1
    fi
done
IDENTIFIER=cc.orbiters.obelisk-cli
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$PROJECT_ROOT/crates/obelisk-cli/Cargo.toml" | head -1)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.]+)?$ ]] || { echo "obelisk-cli has no version." >&2; exit 1; }
NAME="obelisk-cli-$VERSION-arm64"

cargo build --locked --manifest-path "$PROJECT_ROOT/Cargo.toml" -p obelisk-cli --release \
    --target aarch64-apple-darwin
NATIVE_TARGET="$PROJECT_ROOT/build/steam-native-target"
NATIVE_MANIFEST="$PROJECT_ROOT/crates/steam-native/Cargo.toml"
CARGO_TARGET_DIR="$NATIVE_TARGET" cargo build --locked --manifest-path "$NATIVE_MANIFEST" \
    --workspace --bin obelisk-steam-host --lib --profile release --target aarch64-apple-darwin
CARGO_TARGET_DIR="$NATIVE_TARGET" cargo build --locked --manifest-path "$NATIVE_MANIFEST" \
    -p obelisk-steam-native --bin obelisk-steam-sdk --profile release --target x86_64-apple-darwin
CARGO_TARGET_DIR="$NATIVE_TARGET" cargo build --locked --manifest-path "$NATIVE_MANIFEST" \
    -p obelisk-steam-discovery-adapter --lib --profile release --target x86_64-apple-darwin
ARM_BUILD="$NATIVE_TARGET/aarch64-apple-darwin/release"
INTEL_BUILD="$NATIVE_TARGET/x86_64-apple-darwin/release"

STAGE="$PROJECT_ROOT/build/cli/$NAME"
rm -rf "$STAGE"
HELPERS="$STAGE/SteamNative"
mkdir -p "$HELPERS"
CLI="$STAGE/obelisk-cli"
HOST="$HELPERS/obelisk-steam-host"
SDK="$HELPERS/obelisk-steam-sdk-x86_64"
ADAPTER="$HELPERS/libobelisk_steam_discovery_adapter.dylib"
cp "$PROJECT_ROOT/target/aarch64-apple-darwin/release/obelisk-cli" "$CLI"
cp "$ARM_BUILD/obelisk-steam-host" "$HOST"
cp "$INTEL_BUILD/obelisk-steam-sdk" "$SDK"
/usr/bin/lipo -create "$ARM_BUILD/libobelisk_steam_discovery_adapter.dylib" \
    "$INTEL_BUILD/libobelisk_steam_discovery_adapter.dylib" -output "$ADAPTER"
cp "$PROJECT_ROOT/LICENSE" "$STAGE/LICENSE"

# The supervisor runs this Intel-only program with --probe to tell whether Rosetta is installed, before a game
# on Rosetta's Wine or one that renders with D3DMetal. It looks for it beside the helpers' folder.
ROSETTA_SOURCE="$PROJECT_ROOT/packaging/rosetta"
ROSETTA_APP="$STAGE/Obelisk Rosetta.app"
mkdir -p "$ROSETTA_APP/Contents/MacOS"
SDK_PATH="$(xcrun --sdk macosx --show-sdk-path)"
xcrun --sdk macosx clang -arch x86_64 -isysroot "$SDK_PATH" "-mmacosx-version-min=${MACOSX_DEPLOYMENT_TARGET:-26.4}" \
    -fobjc-arc -O2 -Wall -Wextra -Werror -framework AppKit "$ROSETTA_SOURCE/main.m" \
    -o "$ROSETTA_APP/Contents/MacOS/ObeliskRosetta"
cp "$ROSETTA_SOURCE/Info.plist" "$ROSETTA_APP/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleIdentifier -string "$IDENTIFIER.rosetta" "$ROSETTA_APP/Contents/Info.plist"

IDENTITY="${CLI_SIGN_IDENTITY:-}"
if [[ -z "$IDENTITY" ]]; then
    IDENTITIES="$(/usr/bin/security find-identity -v -p codesigning 2>/dev/null || true)"
    for KIND in "Developer ID Application" "Apple Development"; do
        IDENTITY="$(sed -n "s/^ *[0-9]*) \([0-9A-F]\{40\}\) \"$KIND.*/\1/p" <<<"$IDENTITIES" | head -1)"
        [[ -n "$IDENTITY" ]] && break
    done
fi

# Signs "$1" with the options that follow, unless the Keychain keeps codesign waiting for a minute, such as
# for a question nobody is there to answer.
sign() {
    local file="$1"
    shift
    /usr/bin/codesign --force --sign "$IDENTITY" "$TIMESTAMP" "$@" "$file" &
    local signing=$!
    # Its sleep ends with it, so nothing outlives the script.
    (
        trap 'kill "$nap" 2>/dev/null' TERM
        sleep 60 &
        nap=$!
        wait "$nap" && kill "$signing" 2>/dev/null
    ) >/dev/null 2>&1 &
    local watchdog=$!
    local status=0
    wait "$signing" || status=$?
    kill "$watchdog" 2>/dev/null || true
    wait "$watchdog" 2>/dev/null || true
    return "$status"
}

SIGNED=no
if [[ -z "$IDENTITY" ]]; then
    echo "No signing identity: set CLI_SIGN_IDENTITY, or add a Developer ID Application or Apple Development one." >&2
else
    # A Developer ID signature is notarized, which needs Apple's secure timestamp.
    if /usr/bin/security find-identity -v -p codesigning | grep -F "$IDENTITY" | grep -q "Developer ID Application"; then
        TIMESTAMP=--timestamp
    else
        TIMESTAMP=--timestamp=none
    fi
    ENTITLEMENTS="$PROJECT_ROOT/packaging/steam-host.entitlements"
    if sign "$HOST" --options runtime --identifier "$IDENTIFIER.steam-host" --entitlements "$ENTITLEMENTS" \
        && sign "$ADAPTER" --identifier "$IDENTIFIER.steam-discovery" \
        && sign "$SDK" --options runtime --identifier "$IDENTIFIER.steam-sdk-x86_64" --entitlements "$ENTITLEMENTS" \
        && sign "$ROSETTA_APP" --options runtime \
        && sign "$CLI" --options runtime --identifier "$IDENTIFIER"; then
        for FILE in "$HOST" "$ADAPTER" "$SDK" "$ROSETTA_APP" "$CLI"; do
            /usr/bin/codesign --verify --strict "$FILE"
        done
        SIGNED=yes
    else
        echo "codesign did not sign with $IDENTITY within a minute, or failed." >&2
    fi
fi

DIST="$PROJECT_ROOT/build/dist"
mkdir -p "$DIST"
ARCHIVE="$DIST/$NAME.tar.gz"
COPYFILE_DISABLE=1 /usr/bin/tar -czf "$ARCHIVE" -C "$PROJECT_ROOT/build/cli" "$NAME"
(cd "$DIST" && /usr/bin/shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
echo "$ARCHIVE"
if [[ "$SIGNED" == yes ]]; then
    echo "Signed with $(/usr/bin/codesign -dvv "$CLI" 2>&1 | sed -n 's/^Authority=//p' | head -1)," \
        "team $(/usr/bin/codesign -dvv "$CLI" 2>&1 | sed -n 's/^TeamIdentifier=//p')."
else
    echo "UNSIGNED: the archive holds the build as the linker left it. Its Keychain item and the Steam host need a signature." >&2
fi
