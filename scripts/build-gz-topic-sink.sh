#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="${repo_root}/target/gz-topic-sink"

IFS=':' read -r -a ament_prefixes <<< "${AMENT_PREFIX_PATH:-}"
for prefix in "${ament_prefixes[@]}"; do
  for pkgconfig_dir in "${prefix}/lib/pkgconfig" "${prefix}"/opt/*/lib/pkgconfig; do
    if [[ -d "${pkgconfig_dir}" ]]; then
      export PKG_CONFIG_PATH="${pkgconfig_dir}${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
    fi
  done
done

if ! pkg-config --exists gz-transport13; then
  echo "gz-transport13 development metadata is unavailable; source the ROS 2 Jazzy environment first" >&2
  exit 1
fi

mkdir -p "$(dirname "${output}")"
c++ -std=c++17 -O2 -DNDEBUG -Wall -Wextra -Wpedantic \
  "${repo_root}/tools/gz-topic-sink.cpp" \
  -o "${output}" \
  $(pkg-config --cflags --libs gz-transport13)

echo "built ${output}"
