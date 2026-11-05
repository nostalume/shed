import base64
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from scripts import update_packages as packages

ROOT = Path(__file__).resolve().parent.parent


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "nix").mkdir()
        for name in ["shed.json", "nix/shed.nix"]:
            shutil.copyfile(ROOT / name, self.root / name)
        self.original = self.contents()
        self.assets = {}
        for name in packages.ARTIFACTS.values():
            data = ("fixture:" + name).encode()
            self.assets[name] = data
            self.assets[name + ".sha256"] = (hashlib.sha256(data).hexdigest() + "  " + name + "\n").encode()

    def contents(self):
        return [(self.root / name).read_bytes() for name in ["shed.json", "nix/shed.nix"]]

    def test_verified_release_updates_both_catalogs_and_preserves_templates(self):
        self.assertTrue(packages.update(self.root, "v0.2.0", self.assets.__getitem__))
        manifest = json.loads(self.contents()[0])
        self.assertEqual(manifest["version"], "0.2.0")
        windows = manifest["architecture"]["64bit"]
        self.assertIn("/v0.2.0/shed-windows-x86_64.exe#/shed.exe", windows["url"])
        self.assertEqual(windows["hash"], hashlib.sha256(self.assets["shed-windows-x86_64.exe"]).hexdigest())
        template = manifest["autoupdate"]["architecture"]["64bit"]
        self.assertIn("/v$version/", template["url"])
        self.assertTrue(template["hash"]["url"].endswith(".exe.sha256"))
        self.assertNotIn("#/", template["hash"]["url"])
        nix = self.contents()[1].decode()
        self.assertIn('version = "0.2.0";', nix)
        for platform, name in packages.ARTIFACTS.items():
            if platform != "windows":
                sri = "sha256-" + base64.b64encode(hashlib.sha256(self.assets[name]).digest()).decode()
                self.assertIn(sri, next(line for line in nix.splitlines() if f'"{platform}"' in line))
        updated = self.contents()
        self.assertFalse(packages.update(self.root, "0.2.0", self.assets.__getitem__))
        self.assertEqual(self.contents(), updated)

    def test_missing_corrupt_or_misnamed_assets_leave_both_catalogs_untouched(self):
        name = "shed-windows-x86_64.exe"
        for broken in [None, b"corrupt", b"0" * 64 + b"  different.exe\n"]:
            assets = dict(self.assets)
            if broken is None:
                del assets[name]
            elif broken == b"corrupt":
                assets[name] = broken
            else:
                assets[name + ".sha256"] = broken
            with self.subTest(broken=broken), self.assertRaises((ValueError, KeyError)):
                packages.update(self.root, "0.2.0", assets.__getitem__)
            self.assertEqual(self.contents(), self.original)

    def test_stale_and_invalid_versions_do_not_fetch_or_modify(self):
        def unexpected_fetch(name):
            self.fail(f"unexpected download: {name}")

        self.assertFalse(packages.update(self.root, "0.1.5", unexpected_fetch))
        for version in ["v01.2.0", "../0.2.0", "0.2.0;command", "0.2.0-rc1"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                packages.update(self.root, version, unexpected_fetch)
        self.assertEqual(self.contents(), self.original)

    def test_invalid_nix_template_does_not_partially_update_scoop(self):
        path = self.root / "nix/shed.nix"
        path.write_text(path.read_text().replace('"aarch64-linux" =', '"wrong-platform" ='))
        before = self.contents()
        with self.assertRaises(ValueError):
            packages.update(self.root, "0.2.0", self.assets.__getitem__)
        self.assertEqual(self.contents(), before)


class ReleaseHelperTests(unittest.TestCase):
    def run_helper(self, shell, failure="", diff_status=1):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bin").mkdir()
            for name in ["Cargo.toml", "Cargo.lock"]:
                shutil.copyfile(ROOT / name, root / name)
            (root / "nix").mkdir()
            shutil.copyfile(ROOT / "nix/shed.nix", root / "nix/shed.nix")
            git = root / "bin/git"
            git.write_text("""#!/bin/sh
printf 'git %s\n' "$*" >> "$RELEASE_LOG"
if [ "$FAILURE" = "$1" ]; then exit 42; fi
case "$1" in
    branch) echo main;;
    diff) exit "$DIFF_STATUS";;
    tag) if [ "$2" = -a ] && [ "$FAILURE" = tag-create ]; then exit 42; fi;;
esac
""")
            cargo = root / "bin/cargo"
            cargo.write_text("""#!/bin/sh
echo cargo >> "$RELEASE_LOG"
if [ "$FAILURE" = cargo ]; then exit 42; fi
""")
            for command in [git, cargo]:
                command.chmod(0o755)
            env = dict(os.environ, PATH=str(root / "bin") + os.pathsep + os.environ["PATH"],
                       RELEASE_LOG=str(root / "log"), FAILURE=failure, DIFF_STATUS=str(diff_status))
            script = ROOT / ("scripts/release.ps1" if shell == "pwsh" else "scripts/release.sh")
            command = [shell, "-NoProfile", "-NonInteractive", "-File"] if shell == "pwsh" else [shell]
            result = subprocess.run(command + [str(script), "0.2.0"], cwd=root, env=env, capture_output=True)
            trace = (root / "log").read_text()
            nix = (root / "nix/shed.nix").read_bytes()
            self.assertEqual(nix, (ROOT / "nix/shed.nix").read_bytes(), "pre-release must not change live Nix metadata")
            return result, trace

    def check_failures(self, shell):
        for failure, diff_status in [("status", 1), ("cargo", 1), ("", 2), ("tag-create", 1)]:
            with self.subTest(shell=shell, failure=failure, diff_status=diff_status):
                result, trace = self.run_helper(shell, failure, diff_status)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertNotIn("git push", trace)
                if failure != "tag-create":
                    self.assertNotIn("git tag -a", trace)
        result, trace = self.run_helper(shell)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("git push --atomic origin HEAD:main v0.2.0", trace)

    def test_bash_stops_before_publication_after_native_failures(self):
        self.check_failures("bash")

    @unittest.skipUnless(shutil.which("pwsh"), "requires existing PowerShell 7.5+")
    def test_powershell_stops_before_publication_after_native_failures(self):
        self.check_failures("pwsh")


if __name__ == "__main__":
    unittest.main()
