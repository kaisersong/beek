"""Exercise the embedded updater with real bundle copies and isolated OS commands."""
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "src/update_install.sh"


class UpdateInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="beek updater '")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.app = self.root / "Applications/BeeK.app"
        self.new = self.root / "Application Support/$(false) 'new'/BeeK.app"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.events = self.root / "events"
        self.log = self.root / "update.log"
        self.home = self.root / "home"
        self.home.mkdir()
        for bundle, content in [(self.app, "old"), (self.new, "new")]:
            (bundle / "Contents/MacOS").mkdir(parents=True)
            (bundle / "Contents/Info.plist").write_text(content)
            binary = bundle / "Contents/MacOS/BeeK"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        self.env = dict(os.environ, HOME=str(self.home),
                        PATH=f"{self.bin}:/usr/bin:/bin:/usr/sbin:/sbin",
                        EVENTS=str(self.events))
        self.mock("codesign", 'case "$1" in -dv) echo "Authority=Developer ID Application: Fixture";; esac')
        self.mock("launchctl", '''echo "launchctl $*" >> "$EVENTS"
case "$1" in
  print) exit "${LOADED:-1}";;
  bootstrap) exit "${BOOTSTRAP:-0}";;
  kickstart) exit "${KICKSTART:-0}";;
esac''')
        self.mock("open", 'echo "open $*" >> "$EVENTS"')

    def mock(self, name, body):
        command = self.bin / name
        command.write_text(f"#!/bin/sh\nset -eu\n{body}\n")
        command.chmod(0o755)

    def agent(self):
        agent = self.home / "Library/LaunchAgents/com.kaisersong.beek.plist"
        agent.parent.mkdir(parents=True)
        agent.write_text("fixture")

    def run_update(self, relaunch=True, parent=2147483647):
        return subprocess.run(
            ["/bin/sh", str(SCRIPT), str(self.app), str(self.new), "9.9.9",
             str(int(relaunch)), str(parent), str(self.log)],
            env=self.env, timeout=25, capture_output=True,
        )

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, self.log.read_text())
        self.assertEqual((self.app / "Contents/Info.plist").read_text(), "new")

    def test_finder_install_opens_without_launch_agent_and_uses_new_inode(self):
        old_inode = (self.app / "Contents/MacOS/BeeK").stat().st_ino
        self.assert_success(self.run_update())
        self.assertNotEqual((self.app / "Contents/MacOS/BeeK").stat().st_ino, old_inode)
        self.assertIn(f"open {self.app}", self.events.read_text())
        self.assertNotIn("bootstrap", self.events.read_text())

    def test_existing_agent_is_reloaded_and_started(self):
        self.agent()
        self.env["LOADED"] = "0"
        self.assert_success(self.run_update())
        events = self.events.read_text()
        self.assertLess(events.index("bootout"), events.index("bootstrap"))
        self.assertIn("kickstart", events)
        self.assertNotIn("open ", events)

    def test_agent_start_failures_fall_back_to_open(self):
        self.agent()
        for failure in ["BOOTSTRAP", "KICKSTART"]:
            with self.subTest(failure=failure):
                self.env[failure] = "1"
                self.assert_success(self.run_update())
                self.assertIn(f"open {self.app}", self.events.read_text())
                del self.env[failure]
                self.events.unlink()

    def test_copy_or_signature_failure_preserves_installed_bundle(self):
        for name in ["ditto", "codesign"]:
            with self.subTest(command=name):
                self.mock(name, "exit 1")
                self.assertNotEqual(self.run_update().returncode, 0)
                self.assertEqual((self.app / "Contents/Info.plist").read_text(), "old")
                self.assertFalse(self.events.exists())
                (self.bin / name).unlink()
        self.assertFalse(list(self.app.parent.glob(".BeeK-update.*")))

    def test_failed_bundle_switch_restores_previous_bundle(self):
        self.mock("mv", '''case "$1" in */.BeeK-update.*/BeeK.app) exit 1;; esac
exec /bin/mv "$@"''')
        self.assertNotEqual(self.run_update().returncode, 0)
        self.assertEqual((self.app / "Contents/Info.plist").read_text(), "old")
        self.assertNotIn("open ", self.events.read_text())

    def test_silent_install_waits_for_old_process_and_does_not_relaunch(self):
        parent = subprocess.Popen(["/bin/sleep", "20"])
        self.addCleanup(lambda: parent.poll() is None and parent.kill())
        updater = subprocess.Popen(
            ["/bin/sh", str(SCRIPT), str(self.app), str(self.new), "9.9.9",
             "0", str(parent.pid), str(self.log)], env=self.env,
        )
        self.addCleanup(lambda: updater.poll() is None and updater.kill())
        time.sleep(0.8)
        self.assertIsNone(updater.poll())
        self.assertEqual((self.app / "Contents/Info.plist").read_text(), "old")
        parent.terminate()
        parent.wait(timeout=5)
        self.assertEqual(updater.wait(timeout=10), 0, self.log.read_text())
        self.assertEqual((self.app / "Contents/Info.plist").read_text(), "new")
        self.assertFalse(self.events.exists())

    @unittest.skipUnless(shutil.which("clang"), "requires macOS command-line tools")
    def test_signed_native_bundle_runs_on_first_launch_after_switch(self):
        # Real Mach-O signing + execution, not a mocked codesign result.
        (self.bin / "codesign").unlink()
        self.mock("security", "exit 0")
        marker = self.root / "first-launch"
        info = {"CFBundleExecutable": "BeeK", "CFBundleIdentifier": "com.kaisersong.beek.update-test",
                "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "LSUIElement": True}
        (self.new / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        source = '#import <AppKit/AppKit.h>\nint main(int n,char **v){@autoreleasepool{[NSApplication sharedApplication];FILE *f=fopen(v[n-1],"w");if(!f)return 1;fputs("started",f);fclose(f);return 0;}}'
        subprocess.run(["clang", "-x", "objective-c", "-framework", "AppKit", "-o", str(self.new / "Contents/MacOS/BeeK"), "-"],
                       input=source, text=True, check=True, capture_output=True)
        import shlex
        self.mock("open", f'exec /usr/bin/open -W "$1" --args {shlex.quote(str(marker))}')
        result = self.run_update()
        self.assertEqual(result.returncode, 0, self.log.read_text())
        subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(self.app)], check=True)
        self.assertEqual(marker.read_text(), "started")


if __name__ == "__main__":
    unittest.main()
