#!/bin/bash
# Builds the engine files obelisk-cli puts over a Voile release's own (crates/game-runtime/src/voile/engine_patch.rs),
# and writes them with their manifest into crates/game-runtime/engine-patch/:
#
#   wsd                         voile-lkd's window server, which tells the Mac's keyboard layout
#   wineobelisk.so              the unix side of voile-wine's wineobelisk.drv, which makes the Windows layout of it
#   virgl_render_server         Neptune's render server for arm64 (DXMT), which takes a resource update up to 1 GiB
#   virgl_render_server-x86_64  the same for x86_64 (D3DMetal, under Rosetta)
#   manifest.json               the release, the sources' commits, and each file's SHA-256 before and after
#
#   [VOILE_LKD=<checkout of Orbiters-cc/voile-lkd, branch orbiters>] \
#   [VOILE_WINE=<checkout of Orbiters-cc/voile-wine, branch orbiters> ZIG=<zig 0.16.0, the one Voile's sources.json names>] \
#   [NEPTUNE=<checkout of Orbiters-cc/neptune, branch orbiters>]   (meson, ninja and pkg-config on PATH) \
#   scripts/build-engine-patch.sh <installed Voile release folder, such as .../Voile/v26.3.0-27.beta1>
#
# Each part is built when its checkout is given (wsd needs VOILE_LKD and VOILE_WINE, which share wsd_protocol.h);
# a part not given keeps the file already checked in, which must be the one the manifest names. The patch is for
# that release only. Its runtime.json names the commits it was built from: each checkout must be those commits plus
# our own, and the files it replaces must be the release's own.
#
# Before anything is written, each part's own sources at the release's commit are built the same way and compared
# with what the release ships: a toolchain that does not rebuild Voile's file does not build ours.
#  - The driver is built alone, with the release's own ntdll.so and win32u.so to link against, the way Voile's
#    build-linux.sh builds Wine (zig cc for x86-64 Linux, glibc 2.36, 16 KiB pages), and compared byte for byte of
#    code and constants.
#  - The render servers are built as Voile's build-linux.sh and Neptune's build-server.sh build them (meson, the
#    release's macOS minimum, the prefix the release's server names as its own path, stripped, signed ad hoc), and
#    compared byte for byte but for LC_UUID and the ad hoc signature: the linker derives the UUID from the debug
#    map before the strip, which names the folders Voile's CI built in, and the signature hashes the UUID.
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RELEASE_DIR="${1:?usage: build-engine-patch.sh <installed Voile release folder>}"
fail() { echo "$*" >&2; exit 1; }
field() { /usr/bin/python3 -c 'import json,sys; d=json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."): d=d[k]
print(d)' "$RELEASE_DIR/runtime.json" "$1"; }
RELEASE="$(field version)"
LINUX="$RELEASE_DIR/linux"
OUT="$PROJECT_ROOT/crates/game-runtime/engine-patch"
WSD="engine/wsd"
DRIVER="wine/lib/wine/x86_64-unix/wineobelisk.so"
SERVER="graphics/libexec/virgl_render_server"
SERVER_X86="graphics/x86_64/libexec/virgl_render_server"
[[ -f "$LINUX/$WSD" && -f "$LINUX/$DRIVER" && -f "$LINUX/$SERVER" && -f "$LINUX/$SERVER_X86" ]] ||
    fail "$RELEASE_DIR is not a Voile release with the Linux engine."
sha() { /usr/bin/shasum -a 256 "$1" | cut -d' ' -f1; }
for CHECKOUT in "${VOILE_LKD:-}" "${VOILE_WINE:-}" "${NEPTUNE:-}"; do
    [[ -z "$CHECKOUT" || -z "$(git -C "$CHECKOUT" status --porcelain)" ]] ||
        fail "$CHECKOUT has changes that are not committed."
done
if [[ -n "${VOILE_LKD:-}" || -n "${VOILE_WINE:-}" ]]; then
    [[ -n "${VOILE_LKD:-}" && -n "${VOILE_WINE:-}" ]] || fail "wsd and the driver are built together: set VOILE_LKD and VOILE_WINE."
    : "${ZIG:?set ZIG to zig 0.16.0}"
    [[ "$("$ZIG" version)" == 0.16.0 ]] || fail "Voile builds its Linux Wine with zig 0.16.0."
fi
if [[ -n "${NEPTUNE:-}" ]]; then
    for tool in meson ninja pkg-config; do command -v "$tool" >/dev/null || fail "Neptune's render server needs $tool on PATH."; done
fi
# Our commits sit on the commits the release was built from.
based_on() { git -C "$1" merge-base --is-ancestor "$2" HEAD 2>/dev/null || [[ "$(git -C "$1" rev-parse HEAD~1)" == "$2" ]]; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/engine-patch.XXXXXX")"
trap '[ -n "${KEEP_WORK:-}" ] || rm -rf "$WORK"' EXIT
mkdir -p "$WORK/out"
BUILT=""

# The manifest's entry for one file already checked in, when that file is the one it names; empty otherwise.
kept() { /usr/bin/python3 -c 'import json,sys,hashlib,os
m=json.load(open(sys.argv[1])); src=sys.argv[2]
for f in m["files"]:
    if f["source"]==src:
        p=os.path.join(os.path.dirname(sys.argv[1]),src)
        if m["voile"]==sys.argv[3] and os.path.exists(p) and hashlib.sha256(open(p,"rb").read()).hexdigest()==f["patched"]:
            print(json.dumps(f).replace("{", "{ ").replace("}", " }"))' "$OUT/manifest.json" "$1" "$RELEASE" 2>/dev/null || true; }
kept_source() { /usr/bin/python3 -c 'import json,sys
m=json.load(open(sys.argv[1])); s=m["sources"].get(sys.argv[2])
print(json.dumps(s).replace("{", "{ ").replace("}", " }") if s else "")' "$OUT/manifest.json" "$1" 2>/dev/null || true; }

# --- wsd and the driver -------------------------------------------------------------------------------------------
if [[ -n "${VOILE_LKD:-}" ]]; then
    LKD_BASE="$(field linux.voile-lkd)"
    WINE_BASE="$(field voile-wine)"
    based_on "$VOILE_LKD" "$LKD_BASE" || fail "$VOILE_LKD is not built on voile-lkd $LKD_BASE, which $RELEASE holds."
    based_on "$VOILE_WINE" "$WINE_BASE" || fail "$VOILE_WINE is not built on voile-wine $WINE_BASE, which $RELEASE holds."
    # Between the release's commit and ours, nothing but the keyboard layout's files may differ: the patch replaces
    # these files and must not carry a change that needs another.
    others="$(git -C "$VOILE_LKD" diff --name-only "$LKD_BASE" HEAD -- . ':!wsd' ':!tests' ':!README.md' ':!docs')"
    [[ -z "$others" ]] || fail "voile-lkd changed outside wsd since $RELEASE: $others"
    others="$(git -C "$VOILE_WINE" diff --name-only "$WINE_BASE" HEAD -- . ':!dlls/wineobelisk.drv' ':!README.md')"
    [[ -z "$others" ]] || fail "voile-wine changed outside wineobelisk.drv since $RELEASE: $others"
    others="$(git -C "$VOILE_WINE" diff --name-only "$WINE_BASE" HEAD -- dlls/wineobelisk.drv/dllmain.c dlls/wineobelisk.drv/unixlib.h)"
    [[ -z "$others" ]] || fail "the driver's Windows side changed, which the patch does not replace: $others"

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
    cp "$WORK/wsd" "$WORK/out/wsd"
    cp "$WORK/driver/wineobelisk.so" "$WORK/out/wineobelisk.so"
    BUILT="$BUILT wsd wineobelisk.so"
    SOURCE_LKD="{ \"repository\": \"https://github.com/Orbiters-cc/voile-lkd\", \"commit\": \"$(git -C "$VOILE_LKD" rev-parse HEAD)\", \"base\": \"$LKD_BASE\", \"license\": \"GPL-3.0-only\" }"
    SOURCE_WINE="{ \"repository\": \"https://github.com/Orbiters-cc/voile-wine\", \"commit\": \"$(git -C "$VOILE_WINE" rev-parse HEAD)\", \"base\": \"$WINE_BASE\", \"license\": \"LGPL-2.1-or-later\" }"
else
    for source in wsd wineobelisk.so; do
        [[ -n "$(kept "$source")" ]] || fail "No $source of $RELEASE is checked in: set VOILE_LKD, VOILE_WINE and ZIG."
        cp "$OUT/$source" "$WORK/out/$source"
    done
    SOURCE_LKD="$(kept_source voile-lkd)"
    SOURCE_WINE="$(kept_source voile-wine)"
fi

# --- Neptune's render servers -------------------------------------------------------------------------------------
# A thin Mach-O's SHA-256 with its LC_UUID zeroed and its code signature cut, and its length so cut.
normal() { /usr/bin/python3 -I -c 'import hashlib,struct,sys
d=bytearray(open(sys.argv[1],"rb").read())
magic,_,_,_,ncmds,_,_,_=struct.unpack_from("<IiiIIIII",d,0)
assert magic==0xFEEDFACF, "not a thin 64-bit Mach-O"
off,end=32,len(d)
for _ in range(ncmds):
    cmd,size=struct.unpack_from("<II",d,off)
    if cmd==0x1B: d[off+8:off+24]=bytes(16)
    if cmd==0x1D: end=struct.unpack_from("<I",d,off+8)[0]
    off+=size
print(hashlib.sha256(d[:end]).hexdigest(),end)' "$1"; }
if [[ -n "${NEPTUNE:-}" ]]; then
    NEPTUNE_BASE="$(field linux.neptune)"
    based_on "$NEPTUNE" "$NEPTUNE_BASE" || fail "$NEPTUNE is not built on neptune $NEPTUNE_BASE, which $RELEASE holds."
    # Only the render server is replaced: nothing of the guest driver (mesa/) may have changed.
    others="$(git -C "$NEPTUNE" diff --name-only "$NEPTUNE_BASE" HEAD -- . ':!virglrenderer' ':!scripts/test-resource-update.sh' ':!README.md')"
    [[ -z "$others" ]] || fail "neptune changed outside its render server since $RELEASE: $others"
    EPOXY_URL=https://download.gnome.org/sources/libepoxy/1.5/libepoxy-1.5.10.tar.xz
    EPOXY_SHA256=072cda4b59dd098bba8c2363a6247299db1fa89411dc221c8b81b8ee8192e623
    curl -fsSL -o "$WORK/libepoxy.tar.xz" "$EPOXY_URL"
    echo "$EPOXY_SHA256  $WORK/libepoxy.tar.xz" | shasum -a 256 -c - >/dev/null || fail "libepoxy's download is not 1.5.10."
    tar -xf "$WORK/libepoxy.tar.xz" -C "$WORK"
    # Voile's cross file for x86_64 (build-linux.sh, build_neptune_x86_64), without its exe_wrapper: no step of
    # these builds runs what it builds, and a Mac without Rosetta could not.
    cat > "$WORK/x86_64-darwin.ini" <<'EOF'
[binaries]
c = ['clang', '-arch', 'x86_64']
cpp = ['clang++', '-arch', 'x86_64']
objc = ['clang', '-arch', 'x86_64']
objcpp = ['clang++', '-arch', 'x86_64']
ar = 'ar'
strip = 'strip'
pkg-config = 'pkg-config'

[properties]
needs_exe_wrapper = true

[host_machine]
system = 'darwin'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF
    # build_server SOURCES ARCH SHIPPED OUT: the render server of the virglrenderer folder SOURCES for ARCH, built
    # for the prefix and macOS minimum of the release's server SHIPPED, stripped and signed as Voile ships it, at OUT
    build_server() {
        local sources="$1" arch="$2" shipped="$3" out="$4" build prefix minimum cross=()
        build="$WORK/build-$arch-$(basename "$out")"
        prefix="$(strings -a "$shipped" | awk '/^\/.*\/libexec\/virgl_render_server$/ && !found {print; found=1}')"
        prefix="${prefix%/libexec/virgl_render_server}"
        [[ -n "$prefix" ]] || fail "$shipped names no path of its own."
        minimum="$(otool -l "$shipped" | awk '/LC_BUILD_VERSION/{found=1} found && /minos/{print $2; exit}')"
        [[ "$arch" == x86_64 ]] && cross=(--cross-file "$WORK/x86_64-darwin.ini")
        # Voile installs libepoxy into the server's own prefix, and the server is linked with an rpath to it, which
        # meson's install removes again: its length still placed every section. libepoxy goes into a folder whose
        # path is exactly as long as that prefix.
        epoxy="$EPOXY_ROOT/$arch"
        while (( ${#epoxy} < ${#prefix} )); do epoxy="${epoxy}x"; done
        (( ${#epoxy} == ${#prefix} )) || fail "The release's prefix $prefix is too short to build beside: set TMPDIR shorter."
        (
            export MACOSX_DEPLOYMENT_TARGET="$minimum"
            if [[ ! -f "$epoxy/lib/pkgconfig/epoxy.pc" ]]; then
                meson setup "$WORK/epoxy-build-$arch" "$WORK/libepoxy-1.5.10" ${cross[@]+"${cross[@]}"} \
                    --prefix="$epoxy" -Dtests=false -Dglx=no -Degl=no -Dx11=false --buildtype=release >/dev/null
                ninja -j 4 -C "$WORK/epoxy-build-$arch" >/dev/null
                meson install --no-rebuild -C "$WORK/epoxy-build-$arch" >/dev/null
            fi
            meson setup "$build" "$sources" ${cross[@]+"${cross[@]}"} --prefix="$prefix" \
                --pkg-config-path="$epoxy/lib/pkgconfig" -Dneptune=true -Dvenus=false -Dvtest=true \
                -Drender-server-mode=process -Drender-server-worker=process -Dtests=false -Dcheck-gl-errors=false \
                --buildtype=release >/dev/null
            ninja -j 4 -C "$build" >/dev/null
            # Installed, as Voile takes it: meson's install drops the build tree's rpaths.
            meson install --no-rebuild -C "$build" --destdir "$build/stage" >/dev/null
        )
        # Signed under its own name, as Voile signs it: an ad hoc signature names the file it was made for.
        local signed="$build/signed/virgl_render_server"
        mkdir -p "$build/signed"
        cp "$build/stage$prefix/libexec/virgl_render_server" "$signed"
        codesign --remove-signature "$signed" 2>/dev/null || true
        strip -S "$signed"
        codesign --sign - "$signed" 2>/dev/null
        cp "$signed" "$out"
        RENDER_BUILD="$build"
    }
    echo "==> the release's own render servers, to check the toolchain"
    EPOXY_ROOT="$(mktemp -d /private/tmp/epoxy.XXXXXX)"
    git -C "$NEPTUNE" worktree add -q --detach "$WORK/neptune-base" "$NEPTUNE_BASE"
    trap 'git -C "$NEPTUNE" worktree remove --force "$WORK/neptune-base" 2>/dev/null; rm -rf "$EPOXY_ROOT"; [ -n "${KEEP_WORK:-}" ] || rm -rf "$WORK"' EXIT
    for arch in arm64 x86_64; do
        shipped="$LINUX/$SERVER"; [[ "$arch" == x86_64 ]] && shipped="$LINUX/$SERVER_X86"
        build_server "$WORK/neptune-base/virglrenderer" "$arch" "$shipped" "$WORK/base-server-$arch"
        [[ "$(normal "$WORK/base-server-$arch")" == "$(normal "$shipped")" ]] ||
            fail "This toolchain does not rebuild the $arch render server $RELEASE ships: no patch written."
    done
    echo "==> virgl_render_server, arm64 and x86_64"
    build_server "$NEPTUNE/virglrenderer" arm64 "$LINUX/$SERVER" "$WORK/out/virgl_render_server"
    # The render server's own checks, against the objects just built (they run on this Mac's arm64).
    "$NEPTUNE/scripts/test-resource-update.sh" "$RENDER_BUILD" | tail -1 | grep -qx "all passed" ||
        fail "Neptune's resource update test failed: no patch written."
    build_server "$NEPTUNE/virglrenderer" x86_64 "$LINUX/$SERVER_X86" "$WORK/out/virgl_render_server-x86_64"
    BUILT="$BUILT virgl_render_server virgl_render_server-x86_64"
    SOURCE_NEPTUNE="{ \"repository\": \"https://github.com/Orbiters-cc/neptune\", \"commit\": \"$(git -C "$NEPTUNE" rev-parse HEAD)\", \"base\": \"$NEPTUNE_BASE\", \"license\": \"GPL-3.0-only\" }"
else
    for source in virgl_render_server virgl_render_server-x86_64; do
        [[ -n "$(kept "$source")" ]] || fail "No $source of $RELEASE is checked in: set NEPTUNE."
        cp "$OUT/$source" "$WORK/out/$source"
    done
    SOURCE_NEPTUNE="$(kept_source neptune)"
fi

cp "$OUT/manifest.json" "$WORK/manifest.kept.json" 2>/dev/null || true
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$WORK/out/"* "$OUT/"
cp "$WORK/manifest.kept.json" "$OUT/manifest.json" 2>/dev/null || true
# A file built here replaces the release's own, which must be Voile's still; one kept keeps its entry.
entry() {
    local found
    found="$(kept "$2")"
    if [[ -n "$found" && "$(sha "$OUT/$2")" == "$(/usr/bin/python3 -c 'import json,sys; print(json.loads(sys.argv[1])["patched"])' "$found")" &&
          ! " $BUILT " == *" $2 "* ]]; then
        echo "    $found"
        return
    fi
    [[ "$(sha "$LINUX/$1")" != "$(sha "$OUT/$2")" ]] || fail "$LINUX/$1 is ours already: give a copy of $RELEASE as Voile ships it."
    echo "    { \"path\": \"$1\", \"source\": \"$2\", \"original\": \"$(sha "$LINUX/$1")\", \"patched\": \"$(sha "$OUT/$2")\" }"
}
cat > "$WORK/manifest.json" <<JSON
{
  "voile": "$RELEASE",
  "sources": {
    "voile-lkd": $SOURCE_LKD,
    "voile-wine": $SOURCE_WINE,
    "neptune": $SOURCE_NEPTUNE
  },
  "files": [
$(entry "$WSD" wsd),
$(entry "$DRIVER" wineobelisk.so),
$(entry "$SERVER" virgl_render_server),
$(entry "$SERVER_X86" virgl_render_server-x86_64)
  ]
}
JSON
mv "$WORK/manifest.json" "$OUT/manifest.json"
/usr/bin/python3 -m json.tool "$OUT/manifest.json" >/dev/null || fail "The manifest written is not JSON."
cat "$OUT/manifest.json"
