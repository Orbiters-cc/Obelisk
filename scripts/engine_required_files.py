#!/usr/bin/env python3
"""The files a game cannot start without in the Voile release our engine files are for, with their sizes, read from that
release's own archive: crates/game-runtime/voile-required.json (voile/required.rs).

Usage: engine_required_files.py <voile-vX.tar.xz>            writes it
       engine_required_files.py --check <voile-vX.tar.xz>    fails when it is not what the archive holds
"""
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent / "crates/game-runtime"
OUT = HERE / "voile-required.json"
# Everything in these folders, and these files.
FOLDERS = ("linux/engine/", "linux/graphics/", "linux/guest/", "linux/steam/linux64/", "linux/windows/", "linux/wine/bin/")
FILES = {
    "bin/wine",
    "bin/wineserver",
    "lib/external/libd3dshared.dylib",
    "lib/external/D3DMetal.framework/D3DMetal",
    "lib/external/D3DMetal.framework/Versions/Current",
    "lib/external/D3DMetal.framework/Versions/A/D3DMetal",
    "linux/root/usr/bin/dash",
    "linux/wine/lib/wine/x86_64-unix/wine",
    "linux/wine/lib/wine/x86_64-unix/ntdll.so",
    "linux/wine/lib/wine/x86_64-unix/nptunix.so",
    "linux/wine/lib/wine/x86_64-unix/wineobelisk.so",
    "linux/wine/lib/wine/x86_64-windows/ntdll.dll",
    "linux/wine/lib/wine/x86_64-windows/nptunix.dll",
}


def plain(text):
    return re.sub(rb"\\([0-7]{3})", lambda m: bytes([int(m[1], 8)]), text.encode()).decode()


def required(archive, release):
    told = subprocess.run(
        ["/usr/bin/tar", "-cf", "-", "--format=mtree", "--options=!all,type,size,mode,link", "@" + str(archive)],
        env={"LC_ALL": "C"}, check=True, capture_output=True, text=True,
    ).stdout
    files = []
    for line in told.splitlines()[1:]:
        name, *words = line.split(" ")
        keys = dict(word.split("=", 1) for word in words)
        path = plain(name).removeprefix(f"./voile-{release}").removeprefix("/")
        if keys["type"] == "dir" or not (path in FILES or path.startswith(FOLDERS)):
            continue
        if keys["type"] == "link":
            files.append({"path": path, "link": plain(keys["link"])})
        else:
            files.append({"path": path, "size": int(keys["size"]), "executable": bool(int(keys["mode"], 8) & 0o100)})
    missing = FILES - {file["path"] for file in files}
    if missing:
        sys.exit(f"the archive does not hold {sorted(missing)}")
    return {"voile": release, "files": sorted(files, key=lambda file: file["path"])}


def main():
    check = sys.argv[1:2] == ["--check"]
    archive = Path(sys.argv[-1]).resolve()
    release = json.loads((HERE / "engine-patch/manifest.json").read_text())["voile"]
    if archive.name != f"voile-{release}.tar.xz":
        sys.exit(f"the engine files are for {release}")
    text = json.dumps(required(archive, release), indent=1) + "\n"
    if check:
        if OUT.read_text() != text:
            sys.exit("voile-required.json is not what the archive holds")
    else:
        OUT.write_text(text)


main()
