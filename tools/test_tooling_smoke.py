"""Offline regression checks for the tool launcher, including real namespace cleanup."""

from pathlib import Path
import re
import subprocess
import tempfile
import time
import unittest
from contextlib import redirect_stderr
import io

from tooling_smoke import ROOT, isolated_command, run_bounded, parse_options


class ToolingTests(unittest.TestCase):
    def test_capture_world_is_whitelisted_and_only_applies_to_renderdoc(self):
        self.assertEqual(parse_options(["renderdoc"]).world, "range")
        for world in ("range", "industrial", "fine-inspection", "fine-industrial"):
            self.assertEqual(parse_options(["renderdoc", "--world", world]).world, world)
        for args in (["renderdoc", "--world"], ["renderdoc", "--world", "../map"],
                     ["renderdoc", "--world", "industrial; false"],
                     ["blender", "--world", "industrial"], ["tracy", "--world", "range"]):
            with self.subTest(arguments=args), redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                parse_options(args)
            self.assertEqual(error.exception.code, 2)

    def test_namespace_has_no_external_interface(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            command = isolated_command(output, ["ip", "-o", "link", "show"])
            run_bounded(command, output, {"PATH": "/usr/bin:/bin"}, timeout=5)
            interfaces = (output / "process.log").read_text().splitlines()
            self.assertEqual(len(interfaces), 1)
            self.assertIn("lo:", interfaces[0])

    def test_failure_cannot_masquerade_as_success(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "command failed"):
                run_bounded(["false"], Path(directory), {"PATH": "/usr/bin:/bin"}, timeout=5)

    def test_child_file_size_limit_is_applied(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            code = "import resource; print(resource.getrlimit(resource.RLIMIT_FSIZE))"
            run_bounded(isolated_command(output, ["python", "-c", code]), output,
                        {"PATH": "/usr/bin:/bin"}, timeout=5)
            self.assertEqual((output / "process.log").read_text().strip(), "(268435456, 268435456)")

    def test_timeout_reaps_a_detached_descendant(self):
        # The sandbox has a private /tmp; persist the marker in its explicit writable mount.
        (ROOT / "target/tooling").mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "target/tooling") as directory:
            output = Path(directory)
            # A new process group must not escape cleanup of the private PID namespace.
            marker = str(output / "alive")
            code = ("import os,time; "
                    "p=os.fork(); "
                    "os.setsid() if p==0 else None; "
                    f"open({marker!r},'w').write(str(p)) if p else None; "
                    "time.sleep(30)")
            command = isolated_command(output, ["python", "-c", code])
            # Keep the fixture output writable without touching the real checkout.
            command[command.index("--chdir") + 1] = str(output)
            with self.assertRaises(subprocess.TimeoutExpired):
                run_bounded(command, output, {"PATH": "/usr/bin:/bin"}, timeout=0.5)
            self.assertTrue((output / "alive").is_file())
            # No process can retain the namespace once bwrap's PID 1 is gone.
            deadline = time.monotonic() + 1
            while True:
                processes = subprocess.run(["pgrep", "-f", "--", re.escape(marker)],
                                           text=True, capture_output=True, timeout=2)
                self.assertIn(processes.returncode, (0, 1))
                if processes.returncode == 1 or time.monotonic() >= deadline:
                    break
                time.sleep(0.01)
            self.assertEqual(processes.returncode, 1, "owned detached descendant survived cleanup")

    def test_renderdoc_file_size_limit_is_bounded_and_applied(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            code = "import resource; print(resource.getrlimit(resource.RLIMIT_FSIZE))"
            command = isolated_command(output, ["python", "-c", code], max_file_bytes=512 * 1024**2)
            run_bounded(command, output, {"PATH": "/usr/bin:/bin"}, timeout=5)
            self.assertEqual((output / "process.log").read_text().strip(), "(536870912, 536870912)")

    def test_file_size_limit_cannot_be_arbitrarily_expanded(self):
        for invalid in (-1, 0, 1, 512 * 1024**2 + 1, 1024**3):
            with self.subTest(limit=invalid), self.assertRaises(ValueError):
                isolated_command(Path("/tmp"), ["true"], max_file_bytes=invalid)


if __name__ == "__main__":
    unittest.main()
