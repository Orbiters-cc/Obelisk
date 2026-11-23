#!/usr/bin/env python3
"""python3 scripts/test_engine_patch_manifest.py"""
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import engine_patch_manifest as m


def manifest_bytes(patched="b" * 64):
    return json.dumps({"voile": "v1", "files": [{"path": "engine/wsd", "source": "wsd", "original": "a" * 64, "patched": patched}]}).encode()


class Inventory(unittest.TestCase):
    def test_only_the_releases_from_before_the_patch_may_have_no_manifest(self):
        for tag in ("cli-v0.1.0", "cli-v0.1.1"):
            self.assertEqual(m.of_release(tag, b"no manifest here"), {"voile": None, "files": []})
        for tag in ("cli-v0.1.2", "cli-v0.1.8", "cli-v0.2.0"):
            with self.assertRaises(m.Failed):
                m.of_release(tag, b"no manifest here")
        # A truncated or rewritten one is a miss too, never an empty release.
        with self.assertRaises(m.Failed):
            m.of_release("cli-v0.1.8", manifest_bytes()[:-5])
        with self.assertRaises(m.Failed):
            m.of_release("cli-v0.1.8", b'{"voile": "v1", "files": []}')
        with self.assertRaises(m.Failed):
            m.of_release("cli-v0.1.8", b'{"voile": "v1", "files": [{"path": "x"}]}')
        self.assertEqual(m.of_release("cli-v0.1.8", b"\x00junk" + manifest_bytes() + b"\xff")["voile"], "v1")

    def test_generations_are_the_exact_sets_the_releases_left(self):
        with tempfile.TemporaryDirectory() as history:
            for tag, digest in (("0.1.2", "b"), ("0.1.10", "c"), ("0.1.3", "b")):
                with open(f"{history}/release-cli-v{tag}.json", "wb") as file:
                    file.write(manifest_bytes(digest * 64))
            # An unreleased commit's file is replaced, and is no generation.
            with open(f"{history}/0123abc.json", "wb") as file:
                file.write(manifest_bytes("d" * 64))
            entries = [{"path": "engine/wsd", "source": "wsd", "original": "a" * 64, "patched": "e" * 64}]
            entries, found = m.with_history(entries, history, "v1")
            self.assertEqual(entries[0]["previous"], ["b" * 64, "c" * 64, "d" * 64])
            self.assertEqual([g["releases"] for g in found], [["0.1.2", "0.1.3"], ["0.1.10"]])
            self.assertEqual(found[1]["files"], {"engine/wsd": "c" * 64})
            with self.assertRaises(m.Failed):
                m.with_history([], history, "v1")


class Publication(unittest.TestCase):
    def folder(self, root, name, content):
        path = os.path.join(root, name)
        os.mkdir(path)
        with open(f"{path}/wsd", "wb") as file:
            file.write(content)
        digest = m.sha(f"{path}/wsd")
        with open(f"{path}/manifest.json", "wb") as file:
            file.write(manifest_bytes(digest))
        return path

    def test_the_new_folder_takes_the_old_one_s_place_in_one_step(self):
        with tempfile.TemporaryDirectory() as root:
            new, dest = self.folder(root, "new", b"new"), self.folder(root, "dest", b"old")
            m.publish(new, dest)
            with open(f"{dest}/wsd", "rb") as file:
                self.assertEqual(file.read(), b"new")
            self.assertEqual(sorted(os.listdir(root)), ["dest", "new"])

    def test_a_failed_publication_leaves_the_folder_in_place(self):
        with tempfile.TemporaryDirectory() as root:
            new, dest = self.folder(root, "new", b"new"), self.folder(root, "dest", b"old")
            # What is published is not what its manifest names.
            with open(f"{new}/wsd", "wb") as file:
                file.write(b"changed")
            with self.assertRaises(m.Failed):
                m.publish(new, dest)
            self.assertTrue(os.path.isfile(f"{dest}/manifest.json"))
            # The exchange itself fails.
            new2 = self.folder(root, "new2", b"new")
            exchange, m.exchange = m.exchange, lambda a, b: (_ for _ in ()).throw(OSError("a test's failure"))
            try:
                with self.assertRaises(OSError):
                    m.publish(new2, dest)
            finally:
                m.exchange = exchange
            with open(f"{dest}/wsd", "rb") as file:
                self.assertEqual(file.read(), b"old")
            self.assertEqual(sorted(os.listdir(root)), ["dest", "new", "new2"])


if __name__ == "__main__":
    unittest.main()
