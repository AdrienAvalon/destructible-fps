#!/usr/bin/env bash
# Offline GUI: open only trusted local traces. No discovery socket on the host network.
set -euo pipefail
fps_script=$(readlink -f -- "${BASH_SOURCE[0]}")
fps_root=$(dirname -- "$(dirname -- "$fps_script")")
fps_artifacts="$fps_root/target/tooling"
fps_tracy="${HOME:?}/.local/opt/tracy-0.14.1/bin/tracy-profiler"
mkdir -p -- "$fps_artifacts/viewer-config/tracy" "$fps_artifacts/viewer-cache"
exec env -i PATH=/usr/bin:/bin LANG="${LANG:-C.UTF-8}" DISPLAY="${DISPLAY:-:0}" \
  XDG_SESSION_TYPE=x11 GSETTINGS_BACKEND=memory \
  XAUTHORITY="${XAUTHORITY:-}" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}" \
  XDG_CONFIG_HOME="$fps_artifacts/viewer-config" XDG_CACHE_HOME="$fps_artifacts/viewer-cache" \
  bwrap --die-with-parent --new-session --unshare-pid --unshare-net \
  --ro-bind / / --bind "$fps_artifacts" "$fps_artifacts" --dev-bind /dev /dev \
  --ro-bind "$fps_root/tools/tracy-viewer.ini" "$fps_artifacts/viewer-config/tracy/tracy.ini" \
  --proc /proc --tmpfs /tmp --ro-bind /tmp/.X11-unix /tmp/.X11-unix \
  --chdir "$fps_root" -- "$fps_tracy" "$@"
