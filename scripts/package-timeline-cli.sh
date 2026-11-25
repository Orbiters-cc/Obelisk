#!/bin/bash
# The developer's obelisk-cli with the timeline generation's switch, ready to run on the Mac whose Steam session it must read:
# the released helpers of the same version, as they are, and this checkout's obelisk-cli beside them, signed by the same team as
# the released one (the Keychain item belongs to that signature). Nothing is published.
#
#   scripts/package-timeline-cli.sh <released obelisk-cli-<version>-arm64.tar.gz> <built obelisk-cli> <out folder>
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCHIVE="${1:?}" BUILT="${2:?}" OUT="${3:?}"
fail() { echo "$*" >&2; exit 1; }
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$PROJECT_ROOT/crates/obelisk-cli/Cargo.toml" | head -1)"
[[ "$(basename "$ARCHIVE")" == "obelisk-cli-$VERSION-arm64.tar.gz" ]] || fail "The archive is not $VERSION's."
(cd "$(dirname "$ARCHIVE")" && shasum -a 256 -c "$(basename "$ARCHIVE").sha256" >/dev/null) || fail "The archive is not its .sha256's."
WORK="$(mktemp -d "${TMPDIR:-/tmp}/timeline-cli.XXXXXX")"; trap 'rm -rf "$WORK"' EXIT
tar -xzf "$ARCHIVE" -C "$WORK"
STAGE="$WORK/obelisk-cli-$VERSION-arm64"
codesign --verify --strict "$STAGE/obelisk-cli" || fail "The released obelisk-cli's signature is not good."
TEAM="$(codesign -dv "$STAGE/obelisk-cli" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
REQUIREMENT="$(codesign -d -r- "$STAGE/obelisk-cli" 2>/dev/null | sed -n 's/^designated => //p')"
# The identity scripts/build-cli.sh signs a release with, named by the caller as for it, and no other: another one's key may ask a
# question on the screen.
IDENTITY="${CLI_SIGN_IDENTITY:?the Developer ID Application identity, by its name}"
[[ "$IDENTITY" == "Developer ID Application: "* ]] || fail "Only a Developer ID Application identity signs this."
cp "$BUILT" "$WORK/try"
codesign --force --sign "$IDENTITY" --timestamp=none --options runtime --identifier cc.orbiters.obelisk-cli "$WORK/try" 2>/dev/null &
SIGNING=$!
for _ in $(seq 1 60); do kill -0 "$SIGNING" 2>/dev/null || break; sleep 1; done
if kill "$SIGNING" 2>/dev/null; then fail "codesign did not sign within a minute: the Keychain may be asking for its key."; fi
wait "$SIGNING" || fail "codesign failed."
# The same program to macOS: what the released one's signature asks for is what this one has.
codesign --verify --strict -R="$REQUIREMENT" "$WORK/try" || fail "Not signed as the released obelisk-cli (team $TEAM): it could not read its Steam session."
mv "$WORK/try" "$STAGE/obelisk-cli"
rm -rf "$OUT"; mkdir -p "$(dirname "$OUT")"; mv "$STAGE" "$OUT"
for file in "$OUT/obelisk-cli" "$OUT/SteamNative/"* ; do codesign --verify --strict "$file"; done
xattr -dr com.apple.quarantine "$OUT" 2>/dev/null || true
echo "$OUT/obelisk-cli: $(shasum -a 256 "$OUT/obelisk-cli" | cut -c1-16), team $TEAM, satisfies the released one's own requirement"
