#!/usr/bin/env python3
"""The timeline generation's checks (scripts/build-timeline-generation.sh), for a developer's measurements only.

  binary <released file> <ours> [<build folder> ...]   ours loads nothing but macOS's own libraries and the generation's, by
                                                        the same load commands as the released file, and names no build tree
  compare <released linux folder> <timeline linux folder> [--json out]
                                                        every entry of the two trees: only the host files may differ
  files <released linux> <timeline linux> <manifest.json>
                                                        the digests that prove which generation a library holds
  manifest <out folder> <release> <released linux> <timeline linux> <sources json>
"""
import hashlib, json, os, re, stat, subprocess, sys

# What only host programs run or load. Anything else that differs is refused: the emulated game would see it.
HOST = {
    "engine/wsd": "the window server",
    "graphics/libexec/virgl_render_server": "the render server",
    "graphics/lib/libvirglrenderer.1.dylib": "the proxy's library",
    "graphics/dxmt-native.framework/Versions/A/dxmt-native": "DXMT, in the render server",
}
# What the emulated game's processes run, load or map.
GUEST = {
    "fexdarwin-run": ("engine/fexdarwin-run",),
    "Wine": ("wine/",),
    "Neptune's guest driver (Direct3D)": ("windows/",),
    "nptunix": ("wine/lib/wine/x86_64-unix/nptunix.so", "wine/lib/wine/x86_64-windows/nptunix.dll"),
    "the Wine driver (wineobelisk)": ("wine/lib/wine/x86_64-unix/wineobelisk.so", "wine/lib/wine/x86_64-windows/wineobelisk.drv"),
    "the guest's root": ("root/",),
    "the guest's libraries": ("guest/",),
    "Valve's client in the guest": ("steam/",),
}
# Literals that are no build tree's: a default socket's name (virgl_test_server), LLVM's and libc's temporary folders.
ALLOWED = {"/tmp/.virgl_test", "/tmp/", "/var/tmp/"}
PATHS = re.compile(rb"/(?:Users|private|Volumes|opt|home|tmp|var/folders|var/tmp|nix|build|workspace)/[\x20-\x7e]*")


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def run(*command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def loads(path):
    """What a Mach-O loads, where it looks, its own name, and its platform."""
    commands, out = run("/usr/bin/otool", "-l", path), dict(load=[], rpath=[], id=None, minos=None, environment=[])
    kind = None
    for line in commands.splitlines():
        line = line.strip()
        if line.startswith("cmd "):
            kind = line[4:]
        elif m := re.match(r"(?:name|path) (.*) \(offset \d+\)", line):
            if kind in ("LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB", "LC_LOAD_UPWARD_DYLIB", "LC_LAZY_LOAD_DYLIB"):
                out["load"].append(m[1])
            elif kind == "LC_RPATH":
                out["rpath"].append(m[1])
            elif kind == "LC_ID_DYLIB":
                out["id"] = m[1]
            elif kind == "LC_DYLD_ENVIRONMENT":
                out["environment"].append(m[1])
        elif kind == "LC_BUILD_VERSION" and line.startswith("minos "):
            out["minos"] = line[6:]
    return out


def binary(released, ours, *builds):
    problems, a, b = [], loads(released), loads(ours)
    for name in b["load"]:
        if not name.startswith(("/usr/lib/", "/System/Library/", "@rpath/")):
            problems.append(f"loads {name}")
    for name in b["rpath"]:
        if not name.startswith(("@loader_path", "@executable_path")):
            problems.append(f"looks in {name}")
    if b["environment"]:
        problems.append(f"sets {b['environment']}")
    for what in ("load", "rpath", "id", "minos"):
        if (sorted(a[what]) if what in ("load", "rpath") else a[what]) != (sorted(b[what]) if what in ("load", "rpath") else b[what]):
            problems.append(f"{what}: {b[what]}, the released file's {a[what]}")
    with open(ours, "rb") as f:
        data = f.read()
    for found in set(m[0].decode() for m in PATHS.finditer(data)):
        if found not in ALLOWED:
            problems.append(f"holds the path {found}")
    for folder in builds:
        if folder.encode() in data:
            problems.append(f"names {folder}")
    signature = subprocess.run(["/usr/bin/codesign", "--verify", "--strict", ours], capture_output=True, text=True)
    if signature.returncode:
        problems.append(signature.stderr.strip())
    if problems:
        sys.exit(f"{ours}:\n  " + "\n  ".join(problems))
    print(f"ok {os.path.basename(ours)}: loads {[n for n in b['load'] if n.startswith('@')] or 'macOS libraries alone'}, looks in {b['rpath']}, "
          f"id {b['id']}, no build path")


def tree(root):
    """Every entry of a tree: its kind, mode, and content."""
    out = {}
    for folder, names, files in os.walk(root):
        for name in names + files:
            path = os.path.join(folder, name)
            st, rel = os.lstat(path), os.path.relpath(path, root)
            if stat.S_ISLNK(st.st_mode):
                out[rel] = ("link", "", os.readlink(path))
            elif stat.S_ISDIR(st.st_mode):
                out[rel] = ("folder", oct(st.st_mode & 0o777), "")
            elif stat.S_ISREG(st.st_mode):
                out[rel] = ("file", oct(st.st_mode & 0o777), sha(path))
            else:
                out[rel] = ("other", oct(st.st_mode), "")
    return out


def digest(entries):
    return hashlib.sha256("\n".join(f"{p} {' '.join(e)}" for p, e in sorted(entries.items())).encode()).hexdigest()


def compare(released, timeline, out=None):
    a, b = tree(released), tree(timeline)
    differing = sorted(p for p in set(a) | set(b) if a.get(p) != b.get(p))
    result = dict(entries=len(a), differing=[], guest={})
    print(f"{len(a)} entries of the released generation, {len(b)} of the timeline's")
    bad = []
    for p in differing:
        same_shape = p in a and p in b and a[p][0] == b[p][0] == "file" and a[p][1] == b[p][1]
        if p not in HOST or not same_shape:
            bad.append(p)
            continue
        print(f"differs, host only: {p} ({HOST[p]}): {a[p][2][:16]} -> {b[p][2][:16]}")
        result["differing"].append(dict(path=p, released=a[p][2], timeline=b[p][2]))
    for name, paths in GUEST.items():
        pick = lambda t: {p: e for p, e in t.items() if any(p == q or (q.endswith("/") and p.startswith(q)) for q in paths)}
        x, y = pick(a), pick(b)
        same = x == y and bool(x)
        result["guest"][name] = dict(entries=len(x), sha256=digest(x), identical=same)
        print(f"guest side, {'IDENTICAL' if same else 'DIFFERENT'}: {name}: {len(x)} entries, {digest(x)[:16]} / {digest(y)[:16]}")
        if not same:
            bad.append(name)
    if bad:
        sys.exit("These may not differ from the released generation: " + ", ".join(bad))
    missing = sorted(set(HOST) - {d["path"] for d in result["differing"]})
    if missing:
        sys.exit("These are not instrumented: " + ", ".join(missing))
    rest = {p: e for p, e in a.items() if p not in HOST}
    result["rest_sha256"] = digest(rest)
    print(f"everything else, {len(rest)} entries, identical: {digest(rest)[:16]}")
    if out:
        with open(out, "w") as f:
            json.dump(result, f, indent=1)
    return result


def manifest(out, release, pristine, released, timeline, sources):
    with open(sources) as f:
        sources = json.load(f)
    comparison = json.load(open(os.path.join(out, "comparison.json")))
    files = []
    for path in sorted(HOST):
        files.append(dict(path=path, source=os.path.basename(path), original=sha(os.path.join(pristine, path)),
                          released=sha(os.path.join(released, path)), timeline=sha(os.path.join(timeline, path))))
    print(json.dumps(dict(voile=release, sources=sources, files=files, guest=comparison["guest"], rest_sha256=comparison["rest_sha256"]), indent=1))


def files(released, timeline, patch):
    """The digests that prove which generation a library holds: every file either changes, and what the timeline's host programs
    load from inside it."""
    with open(patch) as f:
        paths = sorted({f["path"] for f in json.load(f)["files"]} | set(HOST))
    graphics = {}
    for folder, _, names in os.walk(os.path.join(timeline, "graphics")):
        for name in names:
            path = os.path.join(folder, name)
            if os.path.isfile(path) and not os.path.islink(path):
                graphics[os.path.relpath(path, timeline)] = sha(path)
    graphics["engine/wsd"] = sha(os.path.join(timeline, "engine/wsd"))
    print(json.dumps(dict(released={p: sha(os.path.join(released, p)) for p in paths}, timeline={p: sha(os.path.join(timeline, p)) for p in paths},
                          timeline_graphics=graphics), indent=1))


if __name__ == "__main__":
    what, args = sys.argv[1], sys.argv[2:]
    if what == "binary":
        binary(*args)
    elif what == "compare":
        compare(args[0], args[1], args[3] if args[2:3] == ["--json"] else None)
    elif what == "manifest":
        manifest(*args)
    elif what == "files":
        files(*args)
    else:
        sys.exit(__doc__)
