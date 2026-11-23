#!/usr/bin/env python3
"""The engine patch's manifest (crates/game-runtime/engine-patch/manifest.json): what earlier obelisk-cli releases
put in players' runtimes, the manifest that names every file of theirs, and its publication as one step.

  engine_patch_manifest.py collect HISTORY              every cli-v* release of Orbiters-cc/Obelisk, its built-in manifest
  engine_patch_manifest.py manifest OUT LINUX RELEASE HISTORY LKD WINE NEPTUNE    the manifest, on standard output
  engine_patch_manifest.py refresh MANIFEST HISTORY     `previous` and `generations` of a manifest whose files are kept
  engine_patch_manifest.py check OUT                    the folder holds the files its manifest names, and no other
  engine_patch_manifest.py publish NEW DEST             NEW becomes DEST in one step
"""
import ctypes
import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

REPOSITORY = "Orbiters-cc/Obelisk"
# The releases from before the engine patch: the only ones whose obelisk-cli carries no manifest. A release that is
# not named here and whose manifest is not found stops the build: it is a miss of this reader, not an empty release.
BEFORE_THE_PATCH = {"cli-v0.1.0", "cli-v0.1.1"}
FILES = [
    ("engine/wsd", "wsd"),
    ("wine/lib/wine/x86_64-unix/wineobelisk.so", "wineobelisk.so"),
    ("graphics/libexec/virgl_render_server", "virgl_render_server"),
    ("graphics/x86_64/libexec/virgl_render_server", "virgl_render_server-x86_64"),
    ("engine/fexdarwin-run", "fexdarwin-run"),
]


class Failed(Exception):
    pass


def sha(path):
    with open(path, "rb") as file:
        return hashlib.sha256(file.read()).hexdigest()


def built_in(data):
    """The engine patch manifest in an obelisk-cli's bytes, or None."""
    start = 0
    while True:
        at = data.find(b'"voile":', start)
        if at < 0:
            return None
        brace = data.rfind(b"{", 0, at)
        try:
            value, _ = json.JSONDecoder().raw_decode(data[brace:].decode("utf-8", "replace"))
            if isinstance(value, dict) and value.get("files") and isinstance(value.get("voile"), str):
                return value
        except ValueError:
            pass
        start = at + 1


def of_release(tag, data):
    """The manifest a release carries; an empty one for the releases from before the patch, and for no other."""
    found = built_in(data)
    if found is not None:
        if not all({"path", "patched", "original"} <= set(entry) for entry in found["files"]):
            raise Failed(f"The manifest built into {tag} is not one.")
        return found
    if tag in BEFORE_THE_PATCH:
        return {"voile": None, "files": []}
    raise Failed(f"No engine patch manifest was found in the obelisk-cli of {tag}, and it is not one of the releases "
                 f"from before the patch ({', '.join(sorted(BEFORE_THE_PATCH))}).")


def collect(history):
    run = lambda *args: subprocess.run(args, check=True, capture_output=True, text=True).stdout
    tags = run("gh", "release", "list", "--repo", REPOSITORY, "--limit", "1000", "--json", "tagName", "--jq", ".[].tagName").split()
    tags = [tag for tag in tags if tag.startswith("cli-v")]
    if not BEFORE_THE_PATCH <= set(tags):
        raise Failed("The releases listed are not all of them: the first ones are missing.")
    os.makedirs(history, exist_ok=True)
    for tag in tags:
        with tempfile.TemporaryDirectory() as folder:
            run("gh", "release", "download", tag, "--repo", REPOSITORY, "--pattern", "obelisk-cli-*-arm64.tar.gz", "--dir", folder)
            archives = glob.glob(os.path.join(folder, "obelisk-cli-*-arm64.tar.gz"))
            if len(archives) != 1:
                raise Failed(f"{tag} has {len(archives)} archives.")
            run("/usr/bin/tar", "-xzf", archives[0], "-C", folder)
            binaries = glob.glob(os.path.join(folder, "*", "obelisk-cli"))
            if len(binaries) != 1:
                raise Failed(f"The archive of {tag} has {len(binaries)} obelisk-cli.")
            with open(binaries[0], "rb") as file:
                value = of_release(tag, file.read())
        with open(os.path.join(history, f"release-{tag}.json"), "w") as file:
            json.dump(value, file)


def histories(history, release):
    """(release name or None, manifest) of every manifest of `history` for `release`."""
    for name in sorted(os.listdir(history)):
        with open(os.path.join(history, name)) as file:
            value = json.load(file)  # any unreadable one fails the build
        if value.get("voile") == release:
            tag = name[len("release-cli-v"):-len(".json")] if name.startswith("release-cli-v") else None
            yield tag, value


def version(name):
    return [int(part) for part in name.split(".")]


def generations(history, release):
    """What each published release left in a runtime: its files' digests, the releases that left the same together."""
    sets = {}
    for tag, value in histories(history, release):
        if tag is not None:
            files = {entry["path"]: entry["patched"] for entry in value["files"]}
            sets.setdefault(json.dumps(files, sort_keys=True), []).append(tag)
    found = [{"releases": sorted(tags, key=version), "files": json.loads(files)} for files, tags in sets.items()]
    return sorted(found, key=lambda generation: version(generation["releases"][0]))


def with_history(entries, history, release):
    """`entries` with every digest an earlier manifest had at their path, and the published generations."""
    earlier = {}
    for _, value in histories(history, release):
        for entry in value["files"]:
            earlier.setdefault(entry["path"], set()).update([entry["patched"], *entry.get("previous", [])])
    for entry in entries:
        known = earlier.get(entry["path"], set()) | set(entry.get("previous", []))
        entry["previous"] = sorted(known - {entry["patched"], entry["original"]})
    for path in earlier:
        if all(entry["path"] != path for entry in entries):
            raise Failed(f"{path} was patched before and is not now: its earlier files would block every other")
    found = generations(history, release)
    for generation in found:
        for path, digest in generation["files"].items():
            entry = next(entry for entry in entries if entry["path"] == path)
            if digest not in (entry["patched"], *entry["previous"]):
                raise Failed(f"{path}: the file of {generation['releases']} is not listed")
    return entries, found


def text(release, sources, entries, found):
    lines = ["{", f'  "voile": "{release}",', '  "sources": {']
    lines.append(",\n".join(f'    "{k}": ' + json.dumps(v).replace("{", "{ ").replace("}", " }") for k, v in sources.items()))
    lines += ["  },", '  "files": [']
    lines.append(",\n".join("    " + json.dumps(entry) for entry in entries))
    lines += ["  ],", '  "generations": [']
    lines.append(",\n".join("    " + json.dumps(generation) for generation in found))
    lines += ["  ]", "}"]
    written = "\n".join(lines) + "\n"
    json.loads(written)
    return written


def manifest(out, linux, release, history, lkd, wine, neptune):
    sources = {"voile-lkd": json.loads(lkd), "voile-wine": json.loads(wine), "neptune": json.loads(neptune)}
    entries = [
        {"path": path, "source": source, "original": sha(os.path.join(linux, path)), "patched": sha(os.path.join(out, source))}
        for path, source in FILES
    ]
    return text(release, sources, *with_history(entries, history, release))


def refresh(path, history):
    with open(path) as file:
        value = json.load(file)
    written = text(value["voile"], value["sources"], *with_history(value["files"], history, value["voile"]))
    temporary = path + ".new"
    with open(temporary, "w") as file:
        file.write(written)
        file.flush()
        os.fsync(file.fileno())
    os.replace(temporary, path)


def check(out):
    with open(os.path.join(out, "manifest.json")) as file:
        value = json.load(file)
    named = sorted((entry["path"], entry["source"]) for entry in value["files"])
    if named != sorted(FILES):
        raise Failed(f"The manifest names {named}, not every file of the patch once.")
    for entry in value["files"]:
        if sha(os.path.join(out, entry["source"])) != entry["patched"]:
            raise Failed(f"{entry['source']} is not the file the manifest names.")
    expected = sorted([entry["source"] for entry in value["files"]] + ["manifest.json"])
    if sorted(os.listdir(out)) != expected:
        raise Failed(f"The folder holds {sorted(os.listdir(out))}, not {expected}.")


def publish(new, dest):
    """`new`, checked beside `dest`, exchanged with it: there is no moment without a whole folder at `dest`, and a
    failure leaves the one there."""
    parent = os.path.dirname(os.path.abspath(dest))
    staged = tempfile.mkdtemp(prefix=".engine-patch-", dir=parent)
    try:
        copy = os.path.join(staged, "new")
        shutil.copytree(new, copy)
        check(copy)
        if os.path.exists(dest):
            exchange(copy, dest)
        else:
            os.rename(copy, dest)
    finally:
        shutil.rmtree(staged, ignore_errors=True)


def exchange(a, b):
    library = ctypes.CDLL(None, use_errno=True)
    if library.renamex_np(os.fsencode(a), os.fsencode(b), ctypes.c_uint(0x2)) != 0:  # RENAME_SWAP
        raise OSError(ctypes.get_errno(), f"{a} and {b} cannot be exchanged")


COMMANDS = {"collect": collect, "manifest": manifest, "refresh": refresh, "check": check, "publish": publish}


def main(arguments):
    try:
        result = COMMANDS[arguments[0]](*arguments[1:])
    except (Failed, OSError, ValueError, KeyError, StopIteration, subprocess.CalledProcessError) as error:
        print(f"engine patch: {error!r}" if not isinstance(error, Failed) else f"engine patch: {error}", file=sys.stderr)
        return 1
    if result:
        sys.stdout.write(result)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
