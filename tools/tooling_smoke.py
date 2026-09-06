"""Finite Linux tool checks. No installer, daemon, driver change or external network access."""

import argparse
import csv
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
TRACY_COMMIT = "30997d5ca6bb632cc10807a1da8a6d3de0aeeb3c"
TRACY_PREFIX = Path.home() / ".local/opt/tracy-0.14.1/bin"


def run_bounded(command, output, env, timeout=60):
    """Own the group; kill it on timeout, interruption and ordinary exit as a final safeguard."""
    with (output / "process.log").open("ab") as log:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            result = process.wait(timeout=timeout)
            if result:
                raise RuntimeError(f"command failed ({result}); inspect {output / 'process.log'}")
        finally:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def isolated_command(output, command, *, max_file_bytes=256 * 1024**2):
    # Only the reviewed RenderDoc profile admits the larger bound. Never accept an
    # arbitrary caller-provided cap (or an unlimited/negative RLIMIT sentinel).
    if max_file_bytes not in (256 * 1024**2, 512 * 1024**2):
        raise ValueError("unsupported tool file-size limit")
    # The PID namespace reaps even descendants that create a separate process group.
    # /dev and the existing compositor socket are accessible for real local GPU tests.
    return ["bwrap", "--die-with-parent", "--new-session", "--unshare-pid", "--unshare-net",
            "--ro-bind", "/", "/", "--bind", str(output), str(output),
            "--dev-bind", "/dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--ro-bind-try", "/tmp/.X11-unix", "/tmp/.X11-unix",
            "--chdir", str(ROOT), "--", "prlimit", f"--fsize={max_file_bytes}:{max_file_bytes}", "--",
            *map(str, command)]


def tracy_inner(output):
    # Invoked only inside our private network/PID namespace. Never listen on the host's LAN.
    with (output / "client.log").open("w") as log:
        client = subprocess.Popen([str(output / "tracy-client-smoke")], stdout=log, stderr=log)
        try:
            # Observe the compiled client before connecting; expect only a loopback TCP listener.
            deadline = time.monotonic() + 3
            sockets = ""
            while time.monotonic() < deadline:
                sockets = subprocess.check_output(["ss", "-H", "-ltnu"], text=True, timeout=2)
                if sockets.strip():
                    break
                time.sleep(0.02)
            lines = sockets.splitlines()
            if not lines or any(line.split()[0] != "tcp" or
                                line.split()[4] != "127.0.0.1:8086" for line in lines):
                raise RuntimeError(f"unexpected Tracy listeners: {sockets}")
            (output / "listeners.txt").write_text(sockets)
            subprocess.run([str(TRACY_PREFIX / "tracy-capture"), "-a", "127.0.0.1", "-p", "8086",
                            "-o", str(output / "smoke.tracy"), "-s", "3", "-m", "256"],
                           check=True, timeout=15)
            if client.wait(timeout=5):
                raise RuntimeError("standalone Tracy client failed")
            with (output / "zones.csv").open("w") as report:
                subprocess.run([str(TRACY_PREFIX / "tracy-csvexport"), str(output / "smoke.tracy")],
                               stdout=report, check=True, timeout=15)
            with (output / "zones.csv").open() as report:
                rows = list(csv.DictReader(report))
            if not any(row.get("name") == "fps-tool-smoke-work" and row.get("counts") == "200" for row in rows):
                raise RuntimeError("trace does not contain the known instrumented zone")
            after = subprocess.check_output(["ss", "-H", "-ltnu"], text=True, timeout=2)
            if after.strip():
                raise RuntimeError(f"listeners survived the capture: {after}")
            (output / "tracy-result.json").write_text(json.dumps({
                "version": "0.14.1", "commit": TRACY_COMMIT, "zone_rows": len(rows),
                "listener": "127.0.0.1:8086 in isolated network namespace", "listeners_after": 0,
                "scope": "standalone C++ client only; Rust game instrumentation pending",
            }, indent=2) + "\n")
        finally:
            if client.poll() is None:
                client.kill()
            client.wait()


def parse_options(arguments=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tool", choices=("renderdoc", "blender", "tracy"))
    parser.add_argument("--world", choices=("range", "industrial", "fine-inspection"))
    options = parser.parse_args(arguments)
    if options.world is not None and options.tool != "renderdoc":
        parser.error("--world applies only to the RenderDoc game capture")
    options.world = options.world or "range"
    return options


def main():
    options = parse_options()
    base = ROOT / "target/tooling"
    base.mkdir(parents=True, exist_ok=True)
    # Keep captures for examination; refuse uncontrolled accumulation, do not delete user evidence.
    files = list(base.rglob("*"))
    if len(files) > 2000 or sum(p.stat().st_size for p in files if p.is_file()) > 2 * 1024**3:
        raise RuntimeError("tooling evidence exceeds retention budget; archive selected old runs first")
    output = Path(tempfile.mkdtemp(prefix=f"{options.tool}-", dir=base))
    print(f"Tool artifacts: {output}", flush=True)
    # Deliberate allowlist: do not pass service tokens, proxies, shell startup or Python customizations.
    env = {key: os.environ[key] for key in (
        "DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XAUTHORITY", "LANG", "LC_ALL",
    ) if key in os.environ}
    env.update({"PATH": "/usr/bin:/bin", "FPS_TOOL_OUTPUT": str(output), "FPS_TOOL_ROOT": str(ROOT),
                "FPS_TOOL_WORLD": options.world,
                "XDG_CACHE_HOME": str(output / "cache"), "XDG_DATA_HOME": str(output / "data"),
                "XDG_CONFIG_HOME": str(output / "config"), "OMP_NUM_THREADS": "4"})
    config = output / "data/qrenderdoc/UI.config"
    config.parent.mkdir(parents=True)
    config.write_text(json.dumps({"rdocConfigData": 1, "Analytics_TotalOptOut": True,
                                  "CheckUpdate_AllowChecks": False}))
    if options.tool == "renderdoc":
        # Packaged RenderDoc 1.45 supports Vulkan Xlib/XCB, not VK_KHR_wayland_surface.
        # Use the local XWayland Unix socket; the network namespace remains isolated.
        env.pop("WAYLAND_DISPLAY", None)
        env["QT_QPA_PLATFORM"] = "offscreen"
        command = ["qrenderdoc", "--python", str(ROOT / "tools/renderdoc_smoke.py")]
    elif options.tool == "blender":
        command = ["blender", "--background", "--factory-startup", "--disable-autoexec",
                   "--python-exit-code", "1", "--python", str(ROOT / "tools/blender_smoke.py")]
    else:
        source = Path.home() / ".cache/destructible-fps-tracy-0.14.1"
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        if commit != TRACY_COMMIT:
            raise RuntimeError("Tracy source is not the reviewed version")
        subprocess.run(["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", "public"], check=True)
        compile_command = ["c++", "-std=c++20", "-O2", "-pthread", "-I", str(source / "public"),
                           "-DTRACY_ENABLE", "-DTRACY_ON_DEMAND", "-DTRACY_ONLY_LOCALHOST",
                           "-DTRACY_NO_BROADCAST", "-DTRACY_NO_CODE_TRANSFER", "-DTRACY_NO_SYSTEM_TRACING",
                           "-DTRACY_NO_SAMPLING", str(ROOT / "tools/tracy_smoke.cpp"),
                           str(source / "public/TracyClient.cpp"), "-ldl", "-o", str(output / "tracy-client-smoke")]
        run_bounded(isolated_command(output, compile_command), output, env, timeout=60)
        env["FPS_TRACY_INNER"] = "1"
        command = ["python", str(ROOT / "tools/tooling_smoke.py")]
    file_limit = (512 if options.tool == "renderdoc" else 256) * 1024**2
    run_bounded(isolated_command(output, command, max_file_bytes=file_limit), output, env)
    result = output / f"{options.tool}-result.json"
    if not result.is_file():
        raise RuntimeError(f"missing success proof: inspect {output}; process exit alone is insufficient")
    if options.tool == "renderdoc" and "smoke test graphique termine proprement" not in (output / "process.log").read_text():
        raise RuntimeError("captured a frame, but the owned game's smoke did not report successful completion")
    print(result.read_text())


if __name__ == "__main__":
    if os.environ.get("FPS_TRACY_INNER") == "1":
        tracy_inner(Path(os.environ["FPS_TOOL_OUTPUT"]))
    else:
        main()
