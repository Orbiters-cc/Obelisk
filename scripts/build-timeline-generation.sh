#!/bin/bash
# Builds the timeline generation (docs/CLI.md, "The timeline generation"): the engine files of this checkout's release with
# the instrumented host programs in place of four of them, and nothing else different. For a developer's measurements: no
# release carries it, and no obelisk-cli without the feature developer-timeline knows it.
#
#   VOILE_LKD=<Orbiters-cc/voile-lkd, perf-timeline> NEPTUNE=<Orbiters-cc/neptune, perf-timeline> \
#   DXMT=<Orbiters-cc/dxmt, perf-timeline> DXMT_BUILD=<its meson build folder, configured as the branch's docs say> \
#   scripts/build-timeline-generation.sh <Voile release folder as Voile ships it> <out folder>
#
# meson, ninja and pkg-config on PATH. Runs at background priority. Writes into <out folder>: the four files, original/ (the
# released ones that only this generation changes, to put back), comparison.json, the whole trees released/ and timeline/ (clones),
# and crates/game-runtime/engine-patch/timeline.json of this checkout.
#
# Each binary is relocated to load what the released one loads, from inside the generation, then checked: no path into a
# build tree is in any load command or anywhere in its bytes. Nothing the emulated game runs, loads or maps may
# differ from the released generation by a byte: checked over every entry of the two trees, and printed.
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RELEASE_DIR="${1:?usage: build-timeline-generation.sh <Voile release folder> <out folder>}"
OUT="${2:?usage: build-timeline-generation.sh <Voile release folder> <out folder>}"
: "${VOILE_LKD:?}" "${NEPTUNE:?}" "${DXMT:?}" "${DXMT_BUILD:?}"
fail() { echo "$*" >&2; exit 1; }
TOOL="$PROJECT_ROOT/scripts/timeline_generation.py"
PATCH="$PROJECT_ROOT/crates/game-runtime/engine-patch"
field() { /usr/bin/python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."): d=d[k]
print(d)' "$1" "$2"; }
LINUX="$RELEASE_DIR/linux"
RELEASE="$(field "$RELEASE_DIR/runtime.json" version)"
[[ "$RELEASE" == "$(field "$PATCH/manifest.json" voile)" ]] || fail "$RELEASE_DIR is not the release the engine files are for."
for tool in meson ninja pkg-config; do command -v "$tool" >/dev/null || fail "$tool is not on PATH."; done
for CHECKOUT in "$VOILE_LKD" "$NEPTUNE" "$DXMT"; do
    [[ -z "$(git -C "$CHECKOUT" status --porcelain)" ]] || fail "$CHECKOUT has changes that are not committed."
done
# On the sources of the released files, and on nothing that is held back.
LKD_BASE="$(field "$PATCH/manifest.json" sources.voile-lkd.commit)"
NEPTUNE_BASE="$(field "$PATCH/manifest.json" sources.neptune.commit)"
DXMT_BASE="$(field "$RELEASE_DIR/runtime.json" linux.dxmt)"
[[ "$LKD_BASE" == 3a299f2* ]] || fail "The released wsd is not of the 0.1.8 line."
git -C "$VOILE_LKD" merge-base --is-ancestor "$LKD_BASE" HEAD || fail "$VOILE_LKD is not on $LKD_BASE."
git -C "$NEPTUNE" merge-base --is-ancestor "$NEPTUNE_BASE" HEAD || fail "$NEPTUNE is not on $NEPTUNE_BASE."
git -C "$DXMT" merge-base --is-ancestor "$DXMT_BASE" HEAD || fail "$DXMT is not on $DXMT_BASE."
for held in bf0af9b 248bf43; do
    ! git -C "$VOILE_LKD" merge-base --is-ancestor "$held" HEAD 2>/dev/null || fail "$VOILE_LKD has $held, which is held back."
done
others="$(git -C "$VOILE_LKD" diff --name-only "$LKD_BASE" HEAD -- . ':!wsd' ':!tests' ':!tools' ':!README.md' ':!docs')"
[[ -z "$others" ]] || fail "voile-lkd changed outside wsd: $others"
git -C "$VOILE_LKD" diff --quiet "$LKD_BASE" HEAD -- wsd/wsd_protocol.h || fail "wsd_protocol.h changed."
others="$(git -C "$NEPTUNE" diff --name-only "$NEPTUNE_BASE" HEAD -- . ':!virglrenderer')"
[[ -z "$others" ]] || fail "neptune changed outside its render server: $others"
# One recorder, the same in the three.
H="$(shasum -a 256 < "$VOILE_LKD/wsd/otl.h")"
[[ "$H" == "$(shasum -a 256 < "$NEPTUNE/virglrenderer/src/neptune/otl.h")" && "$H" == "$(shasum -a 256 < "$DXMT/src/util/otl.h")" ]] ||
    fail "The three copies of otl.h differ."

mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd -P)"
# TIMELINE_WORK: a folder kept from one run to the next (its builds go on from there).
KEPT="${TIMELINE_WORK:-}"
if [[ -n "$KEPT" ]]; then mkdir -p "$KEPT"; WORK="$KEPT"; else WORK="$(mktemp -d "${TMPDIR:-/tmp}/timeline-generation.XXXXXX")"; fi
WORK="$(cd "$WORK" && pwd -P)"
trap '[ -n "$KEPT" ] || rm -rf "$WORK"' EXIT
rm -rf "$WORK/bin" "$WORK/stage"; : > "$WORK/log"
quiet() { taskpolicy -b "$@" >> "$WORK/log" 2>&1 || { grep -nE "error|FAILED" "$WORK/log" | head -20; fail "a build step failed: $*"; }; }
BIN="$WORK/bin"; mkdir -p "$BIN"
minimum() { otool -l "$1" | awk '/LC_BUILD_VERSION/{found=1} found && /minos/{print $2; exit}'; }
WSD=engine/wsd SERVER=graphics/libexec/virgl_render_server LIBRARY=graphics/lib/libvirglrenderer.1.dylib
BACKEND=graphics/dxmt-native.framework/Versions/A/dxmt-native

echo "==> wsd"
MACOSX_DEPLOYMENT_TARGET="$(minimum "$LINUX/$WSD")" quiet clang -arch arm64 -fobjc-arc -O2 -Wall -Werror -Wno-deprecated-declarations \
    -o "$BIN/wsd" "$VOILE_LKD/wsd/wsd.m" -framework Cocoa -framework Metal -framework QuartzCore -framework Carbon \
    -framework AudioToolbox -framework CoreAudio -framework IOKit
strip -S "$BIN/wsd"

echo "==> the render server and the proxy's library"
EPOXY_SHA256=072cda4b59dd098bba8c2363a6247299db1fa89411dc221c8b81b8ee8192e623
curl -fsSL -o "$WORK/libepoxy.tar.xz" https://download.gnome.org/sources/libepoxy/1.5/libepoxy-1.5.10.tar.xz
echo "$EPOXY_SHA256  $WORK/libepoxy.tar.xz" | shasum -a 256 -c - >/dev/null || fail "libepoxy's download is not 1.5.10."
rm -rf "$WORK/src"; mkdir "$WORK/src"; tar -xf "$WORK/libepoxy.tar.xz" -C "$WORK/src"
# A prefix that is nowhere: the only path these programs are built with (the launcher names every program they start).
PREFIX=/nonexistent/obelisk-timeline
export MACOSX_DEPLOYMENT_TARGET; MACOSX_DEPLOYMENT_TARGET="$(minimum "$LINUX/$SERVER")"
[[ -f "$WORK/epoxy/build.ninja" ]] || quiet meson setup "$WORK/epoxy" "$WORK/src/libepoxy-1.5.10" --prefix="$WORK/epoxy-prefix" -Dtests=false -Dglx=no -Degl=no -Dx11=false --buildtype=release
quiet ninja -j 4 -C "$WORK/epoxy" install
[[ -f "$WORK/server/build.ninja" ]] || quiet meson setup "$WORK/server" "$NEPTUNE/virglrenderer" --prefix="$PREFIX" --pkg-config-path="$WORK/epoxy-prefix/lib/pkgconfig" \
    -Dneptune=true -Dvenus=false -Dvtest=true -Drender-server-mode=process -Drender-server-worker=process -Dtests=false \
    -Dcheck-gl-errors=false --buildtype=release
quiet ninja -j 4 -C "$WORK/server"
quiet meson install --no-rebuild -C "$WORK/server" --destdir "$WORK/stage"
# The render server's own checks, against the objects just built.
"$NEPTUNE/scripts/test-resource-update.sh" "$WORK/server" | tail -1 | grep -qx "all passed" || fail "Neptune's resource update test failed."
cp "$WORK/stage$PREFIX/libexec/virgl_render_server" "$BIN/"
cp -L "$WORK/stage$PREFIX/lib/libvirglrenderer.1.dylib" "$BIN/"
chmod u+w "$BIN"/*
EPOXY="$(otool -L "$BIN/libvirglrenderer.1.dylib" | awk '/libepoxy/{print $1}')"
for rpath in $(otool -l "$BIN/libvirglrenderer.1.dylib" | awk '/LC_RPATH/{r=1} r && /path /{print $2; r=0}'); do
    install_name_tool -delete_rpath "$rpath" "$BIN/libvirglrenderer.1.dylib" 2>/dev/null
done
install_name_tool -id @rpath/libvirglrenderer.1.dylib -change "$EPOXY" @rpath/libepoxy.0.dylib -add_rpath @loader_path \
    "$BIN/libvirglrenderer.1.dylib" 2>/dev/null
for rpath in $(otool -l "$BIN/virgl_render_server" | awk '/LC_RPATH/{r=1} r && /path /{print $2; r=0}'); do
    install_name_tool -delete_rpath "$rpath" "$BIN/virgl_render_server" 2>/dev/null
done
strip -S "$BIN/virgl_render_server" "$BIN/libvirglrenderer.1.dylib" 2>/dev/null

echo "==> DXMT"
quiet ninja -j 6 -C "$DXMT_BUILD" src/dxmt-native/libdxmt-native.dylib
cp "$DXMT_BUILD/src/dxmt-native/libdxmt-native.dylib" "$BIN/dxmt-native"
install_name_tool -id @rpath/dxmt-native.framework/dxmt-native "$BIN/dxmt-native" 2>/dev/null
strip -S "$BIN/dxmt-native" 2>/dev/null

echo "==> signed, and no build tree in any of them"
for file in wsd virgl_render_server libvirglrenderer.1.dylib dxmt-native; do
    codesign --force --sign - "$BIN/$file" 2>/dev/null
done
for pair in "$PATCH/wsd wsd" "$PATCH/virgl_render_server virgl_render_server" "$LINUX/$LIBRARY libvirglrenderer.1.dylib" \
    "$LINUX/$BACKEND dxmt-native"; do
    set -- $pair
    /usr/bin/python3 -I "$TOOL" binary "$1" "$BIN/$2" "$WORK" "$VOILE_LKD" "$NEPTUNE" "$DXMT" "$DXMT_BUILD" "$HOME" || fail "not relocated"
done

echo "==> the two generations"
rm -rf "$OUT/released" "$OUT/timeline" "$OUT/original"
cp -Rc "$LINUX" "$OUT/released" 2>/dev/null || cp -R "$LINUX" "$OUT/released"
put() { cat "$1" > "$2"; }   # (the mode of the file there stays)
/usr/bin/python3 -I -c 'import json,sys
for f in json.load(open(sys.argv[1]))["files"]: print(f["source"], f["path"], f["patched"])' "$PATCH/manifest.json" |
    while read -r source path patched; do
        put "$PATCH/$source" "$OUT/released/$path"
        [[ "$(shasum -a 256 "$OUT/released/$path" | cut -d' ' -f1)" == "$patched" ]] || fail "$source is not the manifest's"
    done
cp -Rc "$OUT/released" "$OUT/timeline" 2>/dev/null || cp -R "$OUT/released" "$OUT/timeline"
put "$BIN/wsd" "$OUT/timeline/$WSD"
put "$BIN/virgl_render_server" "$OUT/timeline/$SERVER"
put "$BIN/libvirglrenderer.1.dylib" "$OUT/timeline/$LIBRARY"
put "$BIN/dxmt-native" "$OUT/timeline/$BACKEND"
/usr/bin/python3 -I "$TOOL" compare "$OUT/released" "$OUT/timeline" --json "$OUT/comparison.json" ||
    fail "The timeline generation differs where it may not."
mkdir "$OUT/original"
cp "$LINUX/$LIBRARY" "$LINUX/$BACKEND" "$OUT/original/"
cp "$BIN/wsd" "$BIN/virgl_render_server" "$BIN/libvirglrenderer.1.dylib" "$BIN/dxmt-native" "$OUT/"
/usr/bin/python3 -I - "$OUT/sources.json" <<PY
import json, subprocess, sys
head = lambda c: subprocess.run(["git", "-C", c, "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
json.dump({"voile-lkd": {"commit": head("$VOILE_LKD"), "base": "$LKD_BASE"}, "neptune": {"commit": head("$NEPTUNE"), "base": "$NEPTUNE_BASE"},
           "dxmt": {"commit": head("$DXMT"), "base": "$DXMT_BASE"}}, open(sys.argv[1], "w"))
PY
/usr/bin/python3 -I "$TOOL" manifest "$OUT" "$RELEASE" "$LINUX" "$OUT/released" "$OUT/timeline" "$OUT/sources.json" > "$WORK/timeline.json"
cp "$WORK/timeline.json" "$OUT/timeline.json"
/usr/bin/python3 -I "$TOOL" files "$OUT/released" "$OUT/timeline" "$PATCH/manifest.json" > "$OUT/engine-files.json"
cp "$WORK/timeline.json" "$PATCH/timeline.json"
echo "==> $PATCH/timeline.json"
cat "$PATCH/timeline.json"
