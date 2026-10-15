#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repo_root}/reports/gui-matrix}"
simopt_bin="${repo_root}/target/release/simopt"

if [[ ! -x "${simopt_bin}" ]]; then
  echo "missing ${simopt_bin}; run cargo build --release --locked first" >&2
  exit 1
fi

"${repo_root}/scripts/build-gz-topic-sink.sh"
mkdir -p "${output_root}"

cases=(
  "camera-lidar-stress-gui"
  "camera-lidar-stress-gui-observer"
  "camera-lidar-stress-gui-compact"
  "camera-lidar-stress-headless"
)

for case_name in "${cases[@]}"; do
  "${simopt_bin}" bench \
    --scenario "${repo_root}/configs/scenarios/${case_name}.json" \
    --output-dir "${output_root}/${case_name}"
done

"${simopt_bin}" suite-matrix \
  --input "${output_root}/camera-lidar-stress-gui/suite.json" \
  --input "${output_root}/camera-lidar-stress-gui-observer/suite.json" \
  --input "${output_root}/camera-lidar-stress-gui-compact/suite.json" \
  --input "${output_root}/camera-lidar-stress-headless/suite.json" \
  --output "${output_root}/matrix.json"

echo "wrote ${output_root}/matrix.json"
