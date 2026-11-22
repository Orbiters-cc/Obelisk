#!/bin/bash
# Builds the engine files obelisk-cli puts over a Voile release's own, so that Windows games get the keyboard layout
# selected on the Mac (crates/game-runtime/src/voile/engine_patch.rs), and writes them with their manifest into
# crates/game-runtime/engine-patch/:
#
#   wsd             voile-lkd's window server, which tells the Mac's keyboard layout
#   wineobelisk.so  the unix side of voile-wine's wineobelisk.drv, which makes the Windows layout of it
#   manifest.json   the release, the sources' commits, and each file's SHA-256 before and after
#
#   VOILE_LKD=<checkout of Orbiters-cc/voile-lkd, branch orbiters> \
#   VOILE_WINE=<checkout of Orbiters-cc/voile-wine, branch orbiters> \
#   ZIG=<zig 0.16.0, the one Voile's sources.json names> \
#   scripts/build-engine-patch.sh <installed Voile release folder, such as .../Voile/v26.3.0-27.beta1>
#
# The patch is for that release only. Its runtime.json names the voile-lkd and voile-wine commits it was built
# from: each checkout must be those commits plus our own, and the files it replaces must be the release's own. The
# driver is built alone, with the release's own ntdll.so and win32u.so to link against, the way Voile's
# build-linux.sh builds Wine (zig cc for x86-64 Linux, glibc 2.36, 16 KiB pages). Before anything is written, the
# release's own driver sources are built the same way and compared with the driver it ships, byte for byte of
# code and constants: a toolchain that does not rebuild Voile's driver does not build ours.
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RELEASE_DIR="${1:?usage: build-engine-patch.sh <installed Voile release folder>}"
: "${VOILE_LKD:?set VOILE_LKD to a voile-lkd checkout}" "${VOILE_WINE:?set VOILE_WINE to a voile-wine checkout}"
: "${ZIG:?set ZIG to zig 0.16.0}"
fail() { echo "$*" >&2; exit 1; }
field() { /usr/bin/python3 -c 'import json,sys; d=json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."): d=d[k]
print(d)' "$RELEASE_DIR/runtime.json" "$1"; }
RELEASE="$(field version)"
LKD_BASE="$(field linux.voile-lkd)"
WINE_BASE="$(field voile-wine)"
WSD="engine/wsd"
DRIVER="wine/lib/wine/x86_64-unix/wineobelisk.so"
LINUX="$RELEASE_DIR/linux"
[[ -f "$LINUX/$WSD" && -f "$LINUX/$DRIVER" ]] || fail "$RELEASE_DIR is not a Voile release with the Linux engine."
[[ "$("$ZIG" version)" == 0.16.0 ]] || fail "Voile builds its Linux Wine with zig 0.16.0."
for CHECKOUT in "$VOILE_LKD" "$VOILE_WINE"; do
    [[ -z "$(git -C "$CHECKOUT" status --porcelain)" ]] || fail "$CHECKOUT has changes that are not committed."
done
# Our commits sit on the commits the release was built from.
based_on() { git -C "$1" merge-base --is-ancestor "$2" HEAD 2>/dev/null || [[ "$(git -C "$1" rev-parse HEAD~1)" == "$2" ]]; }
based_on "$VOILE_LKD" "$LKD_BASE" || fail "$VOILE_LKD is not built on voile-lkd $LKD_BASE, which $RELEASE holds."
based_on "$VOILE_WINE" "$WINE_BASE" || fail "$VOILE_WINE is not built on voile-wine $WINE_BASE, which $RELEASE holds."
# Between the release's commit and ours, nothing but the keyboard layout's files may differ: the patch replaces two
# files and must not carry a change that needs a third.
others="$(git -C "$VOILE_LKD" diff --name-only "$LKD_BASE" HEAD -- . ':!wsd' ':!tests' ':!README.md' ':!docs')"
[[ -z "$others" ]] || fail "voile-lkd changed outside wsd since $RELEASE: $others"
others="$(git -C "$VOILE_WINE" diff --name-only "$WINE_BASE" HEAD -- . ':!dlls/wineobelisk.drv' ':!README.md')"
[[ -z "$others" ]] || fail "voile-wine changed outside wineobelisk.drv since $RELEASE: $others"
others="$(git -C "$VOILE_WINE" diff --name-only "$WINE_BASE" HEAD -- dlls/wineobelisk.drv/dllmain.c dlls/wineobelisk.drv/unixlib.h)"
[[ -z "$others" ]] || fail "the driver's Windows side changed, which the patch does not replace: $others"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/engine-patch.XXXXXX")"
trap '[ -n "${KEEP_WORK:-}" ] || rm -rf "$WORK"' EXIT
export ZIG_GLOBAL_CACHE_DIR="$WORK/zig-cache" ZIG_LOCAL_CACHE_DIR="$WORK/zig-cache"
TARGET=x86_64-linux-gnu.2.36
mkdir -p "$WORK/include" "$WORK/lib"
# The driver takes nothing from Wine's config.h.
printf '#ifndef __WINE_CONFIG_H\n#define __WINE_CONFIG_H\n#endif\n' > "$WORK/include/config.h"
cp "$LINUX/wine/lib/wine/x86_64-unix/ntdll.so" "$LINUX/wine/lib/wine/x86_64-unix/win32u.so" "$WORK/lib/"

# build_driver SOURCES OUT: the unix library of wineobelisk.drv from the folder SOURCES
build_driver() {
    local sources="$1" out="$2" source objects=()
    mkdir -p "$out"
    for source in "$sources"/*.c; do
        grep -q 'pragma makedep unix' "$source" || continue
        "$ZIG" cc -target "$TARGET" -I"$WORK/include" -I"$VOILE_WINE/include" -I"$sources" -D__WINESRC__ \
            -DWINE_UNIX_LIB -D_WIN64 -Wall -Werror -fPIC -fvisibility=hidden -fno-strict-aliasing \
            -fno-stack-protector -fcf-protection=none -fasynchronous-unwind-tables -O2 \
            -c -o "$out/$(basename "${source%.c}").o" "$source"
        objects+=("$out/$(basename "${source%.c}").o")
    done
    "$ZIG" cc -target "$TARGET" -shared -Wl,-soname,wineobelisk.so -Wl,-z,max-page-size=16384 -Wl,--strip-debug \
        -o "$out/wineobelisk.so" "${objects[@]}" "$WORK/lib/ntdll.so" "$WORK/lib/win32u.so"
}
# the bytes of the code and of the constants, without the symbols a strip may have taken
code() { xcrun llvm-objdump -s -j .text -j .rodata -j .data.rel.ro -j .data "$1" | sed '1,2d'; }

echo "==> the release's own driver, to check the toolchain"
mkdir -p "$WORK/base"
for FILE in $(git -C "$VOILE_WINE" ls-tree --name-only "$WINE_BASE" dlls/wineobelisk.drv/); do
    git -C "$VOILE_WINE" show "$WINE_BASE:$FILE" > "$WORK/base/$(basename "$FILE")"
done
build_driver "$WORK/base" "$WORK/base-out"
cmp -s <(code "$WORK/base-out/wineobelisk.so") <(code "$LINUX/$DRIVER") ||
    fail "This toolchain does not rebuild the driver $RELEASE ships: no patch written."

echo "==> wineobelisk.so"
build_driver "$VOILE_WINE/dlls/wineobelisk.drv" "$WORK/driver"
echo "==> wsd"
# As voile-lkd's wsd/build.sh and Voile's build-linux.sh: for the macOS the release's wsd runs on, without debug
# information, signed ad hoc.
MINIMUM="$(otool -l "$LINUX/$WSD" | awk '/LC_BUILD_VERSION/{found=1} found && /minos/{print $2; exit}')"
MACOSX_DEPLOYMENT_TARGET="$MINIMUM" clang -arch arm64 -fobjc-arc -O2 -Wall -Werror -Wno-deprecated-declarations \
    -o "$WORK/wsd" "$VOILE_LKD/wsd/wsd.m" -framework Cocoa -framework Metal -framework QuartzCore -framework Carbon \
    -framework AudioToolbox -framework CoreAudio -framework IOKit
strip -S "$WORK/wsd"
codesign --force --sign - --identifier wsd "$WORK/wsd" 2>/dev/null
cmp -s "$VOILE_LKD/wsd/wsd_protocol.h" "$VOILE_WINE/dlls/wineobelisk.drv/wsd_protocol.h" ||
    fail "wsd and the driver do not share one wsd_protocol.h."

OUT="$PROJECT_ROOT/crates/game-runtime/engine-patch"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$WORK/wsd" "$OUT/wsd"
cp "$WORK/driver/wineobelisk.so" "$OUT/wineobelisk.so"
sha() { /usr/bin/shasum -a 256 "$1" | cut -d' ' -f1; }
cat > "$OUT/manifest.json" <<JSON
{
  "voile": "$RELEASE",
  "sources": {
    "voile-lkd": { "repository": "https://github.com/Orbiters-cc/voile-lkd", "commit": "$(git -C "$VOILE_LKD" rev-parse HEAD)", "base": "$LKD_BASE", "license": "GPL-3.0-only" },
    "voile-wine": { "repository": "https://github.com/Orbiters-cc/voile-wine", "commit": "$(git -C "$VOILE_WINE" rev-parse HEAD)", "base": "$WINE_BASE", "license": "LGPL-2.1-or-later" }
  },
  "files": [
    { "path": "$WSD", "source": "wsd", "original": "$(sha "$LINUX/$WSD")", "patched": "$(sha "$OUT/wsd")" },
    { "path": "$DRIVER", "source": "wineobelisk.so", "original": "$(sha "$LINUX/$DRIVER")", "patched": "$(sha "$OUT/wineobelisk.so")" }
  ]
}
JSON
cat "$OUT/manifest.json"
