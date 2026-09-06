"""Embedded qrenderdoc Python: capture/replay only our finite local game smoke."""

import json
import os
from pathlib import Path
import resource
import sys
import time
import traceback

import renderdoc as rd

output = Path(os.environ["FPS_TOOL_OUTPUT"])
root = Path(os.environ["FPS_TOOL_ROOT"])
control = None
capture = None
replay = None
try:
    world = os.environ.get("FPS_TOOL_WORLD", "range")
    # Fixed reviewed argument strings: no arbitrary environment text reaches ExecuteAndInject.
    arguments = {
        "range": "--world range --showcase-closeup --smoke-seconds 8",
        "industrial": "--world industrial --showcase-closeup --smoke-seconds 8",
    }[world]
    # qrenderdoc has already initialized replay. No global hook or remote replay server.
    options = rd.CaptureOptions()
    options.allowVSync = True
    options.captureCallstacks = False
    options.hookIntoChildren = False
    launched = rd.ExecuteAndInject(
        str(root / "target/release/playable-demo"), str(root),
        arguments, [], str(output / "breach"), options, False,
    )
    if launched.result != rd.ResultCode.Succeeded:
        raise RuntimeError(str(launched.result))
    control = rd.CreateTargetControl("127.0.0.1", launched.ident, "fps-tool-smoke", False)
    if control is None:
        raise RuntimeError("cannot connect to the owned game capture")
    control.QueueCapture(120, 1)
    deadline = time.monotonic() + 35
    filename = None
    while time.monotonic() < deadline and control.Connected():
        message = control.ReceiveMessage(None)
        if message.type == rd.TargetControlMessageType.NewCapture:
            filename = Path(message.newCapture.path)
        time.sleep(0.01)
    if filename is None or filename.parent != output or not filename.is_file():
        raise RuntimeError("no actual local GPU frame captured")
    file_limit = resource.getrlimit(resource.RLIMIT_FSIZE)[0]
    if not 0 < file_limit <= 512 * 1024**2 or filename.stat().st_size >= file_limit:
        raise RuntimeError("capture reached its finite file limit and may be truncated")
    control.Shutdown()
    control = None
    capture = rd.OpenCaptureFile()
    result = capture.OpenFile(str(filename), "", None)
    if result != rd.ResultCode.Succeeded or not capture.LocalReplaySupport():
        raise RuntimeError(f"capture cannot be replayed locally: {result}")
    result, replay = capture.OpenCapture(rd.ReplayOptions(), None)
    if result != rd.ResultCode.Succeeded:
        raise RuntimeError(f"GPU replay failed: {result}")
    properties = replay.GetAPIProperties()
    if properties.pipelineType != rd.GraphicsAPI.Vulkan:
        raise RuntimeError("this smoke requires an actual Vulkan capture")
    pending = list(replay.GetRootActions())
    draws = 0
    while pending:
        action = pending.pop()
        draws += bool(action.flags & rd.ActionFlags.Drawcall)
        pending.extend(action.children)
    if draws < 10 or not replay.GetTextures():
        raise RuntimeError("captured frame did not contain the populated game scene")
    thumbnail = capture.GetThumbnail(rd.FileType.PNG, 1024)
    (output / "breach-thumbnail.png").write_bytes(bytes(thumbnail.data))
    (output / "renderdoc-result.json").write_text(json.dumps({
        "version": rd.GetVersionString(), "capture": filename.name, "world": world,
        "bytes": filename.stat().st_size, "drawcalls": draws,
        "textures": len(replay.GetTextures()), "api": "Vulkan",
        "scope": "instrumented capture and replay, not a release performance benchmark",
    }, indent=2) + "\n")
except Exception:
    (output / "renderdoc-error.txt").write_text(traceback.format_exc())
finally:
    if replay is not None:
        replay.Shutdown()
    if capture is not None:
        capture.Shutdown()
    if control is not None:
        control.Shutdown()
# SystemExit makes qrenderdoc skip its interactive UI and perform normal shutdown.
# Its exit code is not reliable for Python failures; the launcher requires the success artifact.
sys.exit()
