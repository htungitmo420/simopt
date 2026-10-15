#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repo_root}/reports/sensor-isolation}"
simopt_bin="${repo_root}/target/release/simopt"

"${repo_root}/scripts/build-gz-topic-sink.sh"

if [[ ! -x "${simopt_bin}" ]]; then
  echo "missing ${simopt_bin}; run cargo build --release --locked first" >&2
  exit 1
fi

mkdir -p "${output_root}"

cases=(
  "b2-physics-headless"
  "b3-front-camera-headless"
  "b3-dual-camera-headless"
  "b4-gpu-lidar-headless"
  "camera-lidar-stress-headless"
)

for case_name in "${cases[@]}"; do
  "${simopt_bin}" bench \
    --scenario "${repo_root}/configs/scenarios/${case_name}.json" \
    --output-dir "${output_root}/${case_name}"
done

"${simopt_bin}" suite-matrix \
  --input "${output_root}/b2-physics-headless/suite.json" \
  --input "${output_root}/b3-front-camera-headless/suite.json" \
  --input "${output_root}/b3-dual-camera-headless/suite.json" \
  --input "${output_root}/b4-gpu-lidar-headless/suite.json" \
  --input "${output_root}/camera-lidar-stress-headless/suite.json" \
  --output "${output_root}/matrix.json"

echo "wrote ${output_root}/matrix.json"
