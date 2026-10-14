# simopt

`simopt` is an early-stage, measurement-first toolkit for Gazebo Sim and ROS 2
performance work. It records what the host can actually provide, turns monotonic
wall/simulation timestamps into reproducible Real Time Factor (RTF) reports, and
compares a candidate with its baseline.

The current `0.1.0` MVP does **not** tune Gazebo automatically and does not claim
to improve RTF by itself. It implements the portable P0/P1 foundation in
[PLAN.md](PLAN.md).

## Supported scope

- Reference runtime: Ubuntu 24.04, ROS 2 Jazzy, Gazebo Harmonic on x86_64 Linux.
- Secondary runtime: x86_64 WSL2.
- The core analysis library is platform independent. Gazebo, ROS, renderer, and
  GPU probes are optional and report unavailable capabilities explicitly.
- Windows-native builds are useful for developing and testing the analyzer, but
  the Gazebo/ROS runtime should be inspected from Linux or WSL2.
- The collector has passed its headless smoke test on WSL2 with ROS 2 Jazzy and
  Gazebo Sim 8.15.0. This verifies compatibility, not representative drone
  performance.
- `doctor` checks both GLX and the latest Gazebo Ogre2 renderer log when it is
  available, because EGL headless may use a different device from the desktop.

## Quick start

Install Rust 1.85 or newer, then run:

```bash
cargo build --release
cargo run -p simopt-cli -- doctor --output reports/capabilities.json
cargo run -p simopt-cli -- collect \
  --world default \
  --duration-seconds 180 \
  --output reports/default-raw.csv
cargo run -p simopt-cli -- analyze \
  --input reports/default-raw.csv \
  --label default-baseline \
  --output reports/default-baseline.json
```

Input CSV columns are:

```text
wall_time_s,sim_time_s,paused
```

`collect` subscribes to `/world/<name>/stats` through `gz topic --json-output`.
Use `--topic` if the world publishes statistics elsewhere. Both CSV timestamps
are seconds. `wall_time_s` comes from the collector's monotonic clock rather
than Gazebo's `real_time` field.

For an installation without Gazebo, the included CSV exercises the analyzer:

```bash
cargo run -p simopt-cli -- analyze \
  --input examples/rtf-samples.csv \
  --label example \
  --output reports/example.json
```

Set `paused=true` for samples captured while simulation is paused. The analyzer
also detects simulation-clock resets and starts a new segment.

For a repeatable benchmark, use a versioned scenario manifest:

```bash
source /opt/ros/jazzy/setup.bash
cargo run --release -p simopt-cli -- bench \
  --scenario configs/scenarios/minimal-smoke.json \
  --output-dir reports/bench-minimal
```

`bench` launches the argument array without a shell, waits for the declared
Gazebo topics, verifies one real message on every `message_check_topics` entry,
warms up, captures each repetition, terminates the process group it created,
and writes the scenario snapshot, launch logs, raw CSV, per-run JSON, and
`suite.json`. Message checks happen before warm-up, so their CLI decoding work
is outside the measured interval. The suite fingerprints the scenario,
capability snapshot, and declared workload files with SHA-256, and records the
`simopt` version plus Git commit and dirty state when a repository is available.
Each run records the renderer identity written by Gazebo after launch, so an old
Ogre2 log or a WSL renderer change is not silently attributed to the current
measurement. On Linux it also samples CPU time, average CPU-core use, and peak
RSS for the complete launch process group during each capture. Set
`collect_process_metrics` to `false` to measure without this sampler, or adjust
`resource_sample_interval_ms`. The included scenario uses two short repetitions
for smoke testing; use at least three longer repetitions for performance
decisions.

The repository-owned camera/GPU-LiDAR workload uses only local primitive assets:

```bash
source /opt/ros/jazzy/setup.bash
cargo run --release -p simopt-cli -- bench \
  --scenario configs/scenarios/camera-lidar-smoke.json \
  --output-dir reports/camera-lidar
```

This reference world checks repeatability and renderer reporting across hosts.
It is not a substitute for the real drone world or its obstacle-avoidance
quality and latency gates.

If the light world remains at RTF 1.0, inspect the suite before drawing a
performance conclusion:

```bash
cargo run --release -p simopt-cli -- inspect-suite \
  --input reports/camera-lidar/suite.json \
  --output reports/camera-lidar/diagnostic.json
```

The repository also includes a 1280x720 60 Hz camera and 2048x64 20 Hz GPU
LiDAR stress profile. Run the headless and GUI profiles on the same machine:

```bash
cargo run --release -p simopt-cli -- bench \
  --scenario configs/scenarios/camera-lidar-stress-headless.json \
  --output-dir reports/stress-headless

cargo run --release -p simopt-cli -- bench \
  --scenario configs/scenarios/camera-lidar-stress-gui.json \
  --output-dir reports/stress-gui

cargo run --release -p simopt-cli -- compare-suites \
  --baseline reports/stress-gui/suite.json \
  --candidate reports/stress-headless/suite.json \
  --minimum-gain-percent 5 \
  --output reports/stress-gui-vs-headless.json
```

`compare-suites` uses median RTF across repetitions and invalidates its
performance gate when workload fingerprints or renderer identities differ. It
also warns when both suites are pinned near RTF 1.0 and therefore cannot expose
headroom.

On a Jazzy installation, run the included end-to-end smoke test from WSL2 or
Ubuntu after building the release binary:

```bash
source /opt/ros/jazzy/setup.bash
cargo build --release
bash scripts/smoke-gazebo.sh
```

The script starts `examples/minimal-world.sdf` headlessly, waits for its stats
topic, warms up for two seconds, records five seconds, writes
`reports/gazebo-smoke.json`, and stops the Gazebo process it created. Set
`SIMOPT_WARMUP_SECONDS` to override the smoke-test warm-up.

It can also probe another world. Arguments are world file, world name, duration,
report label, followed by optional `gz sim` arguments. For example, the sensor
demo installed with the Jazzy vendor package can exercise EGL rendering:

```bash
bash scripts/smoke-gazebo.sh \
  /opt/ros/jazzy/opt/gz_sim_vendor/share/gz/gz-sim8/worlds/sensors_demo.sdf \
  lidar_sensor 10 sensors-demo --headless-rendering
```

Set a comma-separated `SIMOPT_SUBSCRIBE_TOPICS` list to require actual messages
and keep consumers connected while collecting RTF. This mode includes Gazebo
CLI subscription and serialization overhead, so use it as a compatibility
check rather than a clean sensor benchmark:

```bash
SIMOPT_SUBSCRIBE_TOPICS=/camera,/lidar \
  bash scripts/smoke-gazebo.sh \
  /opt/ros/jazzy/opt/gz_sim_vendor/share/gz/gz-sim8/worlds/sensors_demo.sdf \
  lidar_sensor 10 sensors-subscribed --headless-rendering
```

To compare a candidate with a baseline:

```bash
cargo run -p simopt-cli -- compare \
  --baseline reports/baseline.json \
  --candidate reports/candidate.json \
  --minimum-gain-percent 10 \
  --output reports/comparison.json
```

The performance gate only evaluates RTF. A Gazebo configuration must also pass
the workload's sensor, latency, and safety/mission quality gates before adoption.
Versioned JSON contracts live in [`schemas/`](schemas/).
Platform-specific evidence is tracked in [COMPATIBILITY.md](COMPATIBILITY.md).

## Current limitations

- The stats collector uses Gazebo CLI JSON output as an initial adapter. It has
  unit tests and a WSL2 Jazzy/Harmonic smoke test, but still needs validation
  against full camera and LiDAR drone workloads and native Linux.
- The benchmark process-group cleanup is implemented for Unix. Windows-native
  development can analyze reports, while Gazebo runtime benchmarks currently
  target Linux and WSL2.
- External probes time out after five seconds; `--skip-probes` avoids them in
  restricted CI environments.
- CPU/RAM detail is richest on Linux. GPU detection does not prove that OGRE is
  using the detected device; use the per-run `renderer` field as evidence for
  the measured Gazebo launch.
- Linux process metrics scan `/proc` at a configurable interval and therefore
  have small measurement overhead. Run a sampler-disabled control when the
  expected performance difference is close to benchmark noise.
- No Rust compute kernel or C++ FFI adapter exists until profiling identifies a
  kernel worth porting.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for performance evidence requirements.

## License

Licensed under the MIT license.

