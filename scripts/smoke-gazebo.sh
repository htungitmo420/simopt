#!/usr/bin/env bash
set -eo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -f /opt/ros/jazzy/setup.bash ]]; then
  # shellcheck disable=SC1091
  source /opt/ros/jazzy/setup.bash
fi

# ROS setup scripts read optional variables that may be unset, so enable this
# stricter check only after the environment has been loaded.
set -u

if ! command -v ros2 >/dev/null 2>&1; then
  echo "ros2 was not found after sourcing /opt/ros/jazzy/setup.bash" >&2
  exit 1
fi

if ! command -v gz >/dev/null 2>&1; then
  echo "gz was not found after sourcing /opt/ros/jazzy/setup.bash" >&2
  exit 1
fi

mkdir -p reports

world_file="${1:-examples/minimal-world.sdf}"
world_name="${2:-simopt_smoke}"
duration_seconds="${3:-5}"
label="${4:-gazebo-smoke}"
warmup_seconds="${SIMOPT_WARMUP_SECONDS:-2}"
subscription_topics_csv="${SIMOPT_SUBSCRIBE_TOPICS:-}"
gz_extra_args=()
if (( $# > 4 )); then
  gz_extra_args=("${@:5}")
fi

if [[ ! "$label" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "label may contain only letters, numbers, dot, underscore, and dash" >&2
  exit 1
fi

gazebo_log="reports/${label}.log"
samples_csv="reports/${label}.csv"
report_json="reports/${label}.json"

gz sim -s -r "${gz_extra_args[@]}" "$world_file" >"$gazebo_log" 2>&1 &
gazebo_pid=$!
subscriber_pids=()

cleanup() {
  for subscriber_pid in "${subscriber_pids[@]}"; do
    if kill -0 "$subscriber_pid" 2>/dev/null; then
      kill "$subscriber_pid" 2>/dev/null || true
    fi
    wait "$subscriber_pid" 2>/dev/null || true
  done
  if kill -0 "$gazebo_pid" 2>/dev/null; then
    kill "$gazebo_pid" 2>/dev/null || true
  fi
  wait "$gazebo_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

stats_topic="/world/${world_name}/stats"
topic_ready=false
for _ in $(seq 1 60); do
  if gz topic -l 2>/dev/null | grep -Fqx "$stats_topic"; then
    topic_ready=true
    break
  fi

  if ! kill -0 "$gazebo_pid" 2>/dev/null; then
    echo "Gazebo stopped before publishing $stats_topic" >&2
    cat "$gazebo_log" >&2
    exit 1
  fi

  sleep 0.25
done

if [[ "$topic_ready" != true ]]; then
  echo "Timed out waiting for $stats_topic" >&2
  cat "$gazebo_log" >&2
  exit 1
fi

echo "warming up Gazebo for ${warmup_seconds}s"
sleep "$warmup_seconds"

if [[ -n "$subscription_topics_csv" ]]; then
  IFS=',' read -r -a subscription_topics <<<"$subscription_topics_csv"
  for subscription_topic in "${subscription_topics[@]}"; do
    if [[ -z "$subscription_topic" ]]; then
      continue
    fi

    if ! timeout 10s bash -c \
      'gz topic -e -t "$1" | head -c 1 >/dev/null' \
      bash "$subscription_topic"; then
      echo "No data received from required topic: $subscription_topic" >&2
      exit 1
    fi
    echo "received data from $subscription_topic"

    # Keep a consumer connected during collection. This intentionally includes
    # gz CLI transport/serialization cost and is a compatibility smoke test,
    # not a clean sensor microbenchmark.
    gz topic -e -t "$subscription_topic" >/dev/null 2>&1 &
    subscriber_pids+=("$!")
  done
fi

./target/release/simopt collect \
  --world "$world_name" \
  --duration-seconds "$duration_seconds" \
  --output "$samples_csv"

./target/release/simopt analyze \
  --input "$samples_csv" \
  --label "$label" \
  --output "$report_json"

echo "Smoke test complete"
echo "  samples: $samples_csv"
echo "  report:  $report_json"
echo "  log:     $gazebo_log"
