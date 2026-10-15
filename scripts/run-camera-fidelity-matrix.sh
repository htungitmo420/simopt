#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repo_root}/reports/camera-fidelity-matrix}"
simopt_bin="${repo_root}/target/release/simopt"
cooldown_seconds="${SIMOPT_MATRIX_COOLDOWN_SECONDS:-10}"

"${repo_root}/scripts/build-gz-topic-sink.sh"

if [[ ! -x "${simopt_bin}" ]]; then
  echo "missing ${simopt_bin}; run cargo build --release --locked first" >&2
  exit 1
fi

mkdir -p "${output_root}"

run_case() {
  local scenario_name="$1"
  local output_name="$2"
  "${simopt_bin}" bench \
    --scenario "${repo_root}/configs/scenarios/${scenario_name}.json" \
    --output-dir "${output_root}/${output_name}"
  sleep "${cooldown_seconds}"
}

# The repeated reference at the end exposes temperature or host-load drift.
run_case camera-lidar-stress-gui-compact reference-start
run_case camera-lidar-stress-gui-camera-rate camera-rate
run_case camera-lidar-stress-gui-camera-resolution camera-resolution
run_case camera-lidar-stress-gui-camera-balanced camera-balanced
run_case camera-lidar-stress-gui-compact-control reference-end

"${simopt_bin}" suite-matrix \
  --input "${output_root}/reference-start/suite.json" \
  --input "${output_root}/camera-rate/suite.json" \
  --input "${output_root}/camera-resolution/suite.json" \
  --input "${output_root}/camera-balanced/suite.json" \
  --input "${output_root}/reference-end/suite.json" \
  --output "${output_root}/matrix.json"

echo "wrote ${output_root}/matrix.json"
