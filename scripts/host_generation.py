#!/usr/bin/env python3
"""A developer generation that is the released one with other programs on the Mac's side, for a start by hand before they are released.
  host_generation.py <Voile release folder, as Voile ships it> <engine-patch folder of the released set> <engine-patch folder with
                     the new files> <out folder> [<sources>=<commit> ...]
Writes into <out>: released/ and timeline/ (whole trees, clones; "timeline" is the other generation's name in the developer's
obelisk-cli), the new files, original/ (empty: no file that only this generation changes), comparison.json (every entry of the two
trees compared), timeline.json (what the developer's obelisk-cli is built with), engine-files.json (the digests that prove which is
in place) and sources.json. Nothing is written when anything but HOST differs, and every new file must differ: no file that the game
runs, loads or maps is among them. Reads nothing of the library and writes nothing to it."""
import hashlib, json, os, shutil, stat, subprocess, sys
HOST = {"engine/wsd": "wsd", "graphics/libexec/virgl_render_server": "virgl_render_server",
        "graphics/x86_64/libexec/virgl_render_server": "virgl_render_server-x86_64"}


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def tree(root):
    out = {}
    for folder, names, files in os.walk(root):
        for name in names + files:
            path = os.path.join(folder, name); st = os.lstat(path); rel = os.path.relpath(path, root)
            if stat.S_ISLNK(st.st_mode): out[rel] = ("link", "", os.readlink(path))
            elif stat.S_ISDIR(st.st_mode): out[rel] = ("folder", oct(st.st_mode & 0o777), "")
            elif stat.S_ISREG(st.st_mode): out[rel] = ("file", oct(st.st_mode & 0o777), sha(path))
            else: out[rel] = ("other", oct(st.st_mode), "")
    return out


def put(source, to):  # (the mode of the file there stays)
    with open(source, "rb") as f, open(to, "wb") as t:
        t.write(f.read())


def main():
    release, patch, new, out = (os.path.abspath(p) for p in sys.argv[1:5])
    with open(os.path.join(release, "runtime.json")) as f: voile = json.load(f)["version"]
    with open(os.path.join(patch, "manifest.json")) as f: manifest = json.load(f)
    if manifest["voile"] != voile: sys.exit(f"the engine files are for {manifest['voile']}")
    for source in HOST.values():
        if subprocess.run(["/usr/bin/codesign", "--verify", "--strict", os.path.join(new, source)]).returncode: sys.exit(f"{source} is not signed")
    os.makedirs(out, exist_ok=True)
    for name in ("released", "timeline", "original"):
        shutil.rmtree(os.path.join(out, name), ignore_errors=True)
    linux, released, other = os.path.join(release, "linux"), os.path.join(out, "released"), os.path.join(out, "timeline")
    subprocess.run(["/bin/cp", "-Rc", linux, released], check=True)
    for f in manifest["files"]:
        put(os.path.join(patch, f["source"]), os.path.join(released, f["path"]))
        if sha(os.path.join(released, f["path"])) != f["patched"]: sys.exit(f"{f['source']} is not the manifest's")
    subprocess.run(["/bin/cp", "-Rc", released, other], check=True)
    for path, source in HOST.items():
        put(os.path.join(new, source), os.path.join(other, path))
        shutil.copy(os.path.join(new, source), os.path.join(out, source))
    os.mkdir(os.path.join(out, "original"))
    a, b = tree(released), tree(other)
    differing = sorted(p for p in set(a) | set(b) if a.get(p) != b.get(p))
    if differing != sorted(HOST) or any(a[p][:2] != b[p][:2] for p in HOST): sys.exit(f"these differ: {differing}")
    rest = hashlib.sha256("\n".join(f"{p} {' '.join(e)}" for p, e in sorted(a.items()) if p not in HOST).encode()).hexdigest()
    entries = {f["path"]: f for f in manifest["files"]}
    paths = sorted(entries)
    graphics = {p: e[2] for p, e in b.items() if e[0] == "file" and (p.startswith("graphics/") or p == "engine/wsd")}
    guest = {p: a[p][2] for p in sorted(a) if a[p][0] == "file" and not p.startswith(("graphics/", "engine/"))}
    sources = dict(s.split("=", 1) for s in sys.argv[5:])
    def write(name, what):
        with open(os.path.join(out, name), "w") as f: json.dump(what, f, indent=1)
    write("comparison.json", dict(entries=len(a), differing=[dict(path=p, released=a[p][2], timeline=b[p][2]) for p in differing], rest_sha256=rest,
                                  guest_files=len(guest), guest_sha256=hashlib.sha256("\n".join(f"{p} {d}" for p, d in guest.items()).encode()).hexdigest(),
                                  fexdarwin_run=a["engine/fexdarwin-run"][2], wineobelisk=a["wine/lib/wine/x86_64-unix/wineobelisk.so"][2]))
    write("sources.json", sources)
    write("timeline.json", dict(voile=voile, sources=sources, files=[dict(path=p, source=HOST[p], original=entries[p]["original"], released=entries[p]["patched"],
                                timeline=b[p][2]) for p in sorted(HOST)], guest={}, rest_sha256=rest))
    write("engine-files.json", dict(released={p: a[p][2] for p in paths}, timeline={p: b[p][2] for p in paths}, timeline_graphics=graphics))
    print(f"{len(a)} entries; differ: " + ", ".join(f"{p} {a[p][2][:12]} -> {b[p][2][:12]}" for p in differing) + f"; everything else identical: {rest[:16]}")


main()
