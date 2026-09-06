#!/usr/bin/env bash
# Reproducible local checkpoint gates; no deploy, push, asset fetch or permission changes.
set -euo pipefail

fps_gpu=false
if (( $# > 1 )); then
  printf '%s\n' 'Too many arguments' >&2
  exit 64
fi
case "${1:-}" in
  "") ;;
  --gpu) fps_gpu=true ;;
  --help) printf '%s\n' 'Usage: bash tools/validate_checkpoint.sh [--gpu]' 'CPU gates always run; --gpu additionally requires a free native Vulkan desktop.'; exit 0 ;;
  *) printf '%s\n' 'Unsupported argument' >&2; exit 64 ;;
esac
fps_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd -- "$fps_root"
command -v cargo >/dev/null
command -v python3 >/dev/null
command -v timeout >/dev/null
mkdir -p -- target
fps_logs=$(mktemp -d "$fps_root/target/checkpoint-validation-XXXXXX")
printf 'Local validation logs: %s\n' "$fps_logs"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
fps_run() {
  local fps_name=$1
  shift
  printf 'Running %s\n' "$fps_name"
  if "$@" >"$fps_logs/$fps_name.log" 2>&1; then
    printf '%s\t0\n' "$fps_name" >>"$fps_logs/results.tsv"
  else
    local fps_code=$?
    printf '%s\t%s\n' "$fps_name" "$fps_code" >>"$fps_logs/results.tsv"
    printf 'FAILED %s (exit %s); inspect its local log.\n' "$fps_name" "$fps_code" >&2
    return "$fps_code"
  fi
}
fps_run fmt cargo fmt --check
fps_run clippy cargo clippy --locked --all-targets -- -D warnings
fps_run debug cargo test --locked --all-targets
fps_run release cargo test --locked --release --all-targets
fps_run doctests cargo test --locked --doc
for fps_suite in network secure_transport secure_authority secure_server_process; do
  fps_run "$fps_suite" cargo test --locked --test "$fps_suite"
done
fps_run oidc cargo test --locked oidc::tests
fps_run python python3 -m unittest discover -s tools -p 'test_*.py'
fps_run destruction cargo run --locked --release --bin destruction-benchmark -- --events 500
fps_run structural cargo run --locked --release --bin structural-benchmark -- --iterations 100
fps_run physics cargo run --locked --release --bin physics-benchmark -- --bodies 1024 --ticks 300
fps_run snapshot cargo run --locked --release --bin snapshot-benchmark -- --iterations 20
if "$fps_gpu"; then
  if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    printf '%s\n' 'No native display: GPU gates NOT validated.' >&2
    exit 69
  fi
  for fps_profile in debug release; do
    fps_flags=()
    if [[ "$fps_profile" == release ]]; then fps_flags=(--release); fi
    fps_run "gpu-lib-$fps_profile" cargo test --locked "${fps_flags[@]}" --lib -- --ignored --nocapture
    fps_run "gpu-material-$fps_profile" cargo test --locked "${fps_flags[@]}" --test material_projection -- --ignored --nocapture
    fps_run "gpu-raster-$fps_profile" cargo test --locked "${fps_flags[@]}" --test finish_raster -- --ignored --nocapture
  done
fi
# Rebuild standalone executables after the release-test harness artifacts.
fps_run native-build cargo build --locked --release --bin playable-demo --bin fine-geometry-demo --bin fine-mesh-benchmark
fps_run binary-hashes sha256sum target/release/playable-demo target/release/fine-geometry-demo target/release/fine-mesh-benchmark
if "$fps_gpu"; then
  fps_run playable timeout --kill-after=5s 60s target/release/playable-demo --smoke-seconds 5
  fps_run exact timeout --kill-after=5s 60s target/release/fine-geometry-demo --smoke-seconds 8
  for fps_view in fracture approach wide; do
    fps_run "industrial-$fps_view" timeout --kill-after=5s 60s target/release/fine-geometry-demo --world industrial --view "$fps_view" --smoke-seconds 12
  done
  printf '%s\n' 'CPU and native GPU gates completed; inspect images separately. These are not performance acceptance.'
else
  printf '%s\n' 'CPU gates completed. GPU/rendering and visual acceptance NOT validated by this run.'
fi
