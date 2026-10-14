# Implementation status

Updated: 2026-10-13

## Implemented

- Cargo workspace with portable `simopt-core` and `simopt` CLI.
- `simopt doctor`: WSL/native/container classification, CPU features, memory,
  and optional ROS 2, Gazebo, renderer, NVIDIA, and `perf` probes.
- `simopt collect`: captures `gz.msgs.WorldStatistics` JSON from
  `/world/<world>/stats`, timestamps arrivals with a monotonic clock, and writes
  CSV without modifying the world.
- `simopt analyze`: excludes pause/reset/non-advancing intervals and reports
  total, median, p05, p95, min, and max RTF.
- `simopt compare`: computes relative improvement and a configurable performance
  gate. Mission/sensor quality remains a separate required gate.
- `simopt bench`: loads a versioned JSON scenario, launches an argument array
  without a shell, waits for required Gazebo topics and optional real messages,
  warms up, repeats fixed wall-duration captures, cleans up its process group,
  and writes per-run and aggregate reports.
- Optional Linux process-group monitoring records CPU time, average CPU-core
  use, and peak RSS during each measured interval.
- Each repeated suite includes SHA-256 fingerprints for its scenario,
  capability snapshot, and declared workload files, plus tool version and Git
  source revision when available.
- Each run records a fresh Gazebo Ogre2 renderer identity and warns on software
  rendering; suite aggregation warns when renderer selection changes between
  repetitions.
- Unit tests, release build, CI workflow, sample data, contribution guidance,
  and MIT license.
- Reusable minimal and camera/GPU-LiDAR headless Gazebo worlds plus a WSL2
  smoke-test script that can require actual sensor messages.
- Suite diagnosis classifies real-time-capped, below-real-time, faster-than-real-time,
  unstable, and inconclusive workloads. Repeated-suite comparison checks workload
  fingerprints and renderer identity before evaluating its performance gate.
- A heavier camera/GPU-LiDAR reference world and paired GUI/headless scenarios
  provide a portable B0/B1-style experiment before the real drone is available.

## Verified in this workspace

- Rust 1.85.0 on Ubuntu 24.04 WSL2.
- `cargo fmt --all --check`, Clippy with warnings denied, and all 28 unit tests
  pass, including pause/reset segmentation and Gazebo JSON parsing.
- Release CLI executed `doctor`, `analyze`, and `compare` successfully using
  sample data.
- End-to-end collection passed on ROS 2 Jazzy with Gazebo Sim 8.15.0: the
  five-second minimal-world smoke test captured 48 valid samples with no
  exclusions and measured total RTF 0.9999. This is a compatibility test, not
  evidence for camera/LiDAR or drone performance.
- Gazebo's installed multi-sensor demo initialized Ogre2 camera, depth, RGBD,
  GPU LiDAR, and thermal sensors with EGL headless. After a two-second warm-up,
  an eight-second run without consumers captured 79 samples with total RTF
  1.0061, median-window RTF 1.0000, and p05-window RTF 0.9978. The first
  non-warmed run showed why renderer initialization must be excluded from
  performance measurements.
- A second sensor run required real `/camera` and `/lidar` messages and kept two
  `gz topic` consumers connected. It captured 75 stats samples with total RTF
  0.9557, median-window RTF 0.9547, and p05-window RTF 0.9209. This includes CLI
  transport and text-serialization overhead, so it is a compatibility/load
  smoke test rather than a clean sensor microbenchmark.
- The sample comparison is synthetic and is not Gazebo performance evidence.
- The repeated runner completed two end-to-end minimal-world runs and cleaned up
  all child processes. Total RTF was 1.0003 and 0.9992; the suite median was
  0.9997 and p05 across runs was 0.9993. Two short repetitions validate the
  runner but are intentionally below the performance-decision gate.
- A second two-run suite validated process metrics. Each run collected 13
  resource samples, approximately 0.96 seconds of CPU time, 0.32 average CPU
  cores, and about 137 MB peak RSS while total RTF remained near 1.0. Metric
  reports contained no warnings, and process-group cleanup still passed.
- The repository-owned camera/GPU-LiDAR scenario completed two repeated runs
  at total RTF 1.0001 and 1.0002. Each run recorded about 0.39 average CPU cores,
  about 524 MB peak RSS, and a fresh `llvmpipe` renderer identity. The suite
  correctly warns that software-rendered results are not representative of GPU
  acceleration.
- A separate sensor-message smoke run kept consumers attached to
  `/simopt/camera` and `/simopt/lidar`, received data from both topics, and
  measured total RTF 0.9535 with p05 window RTF 0.9269. This includes CLI
  subscription and serialization overhead.
- The scenario-driven runner now verifies one real camera message and one real
  LiDAR message before every repetition. Its two-run WSL2 integration test
  passed, cleaned up all Gazebo/subscriber processes, recorded RTF 1.0001 and
  1.0176, and retained the per-run `llvmpipe` warning.

## Environment findings

The tested WSL2 distro provides ROS 2 Jazzy and Gazebo Sim 8.15.0 after sourcing
`/opt/ros/jazzy/setup.bash`; `perf` is absent. The current host identifies an
AMD Ryzen 5 6600H and NVIDIA RTX 3050 Ti. Controlled camera/GPU-LiDAR runs
record `EGL_MESA_device_software` and `llvmpipe` from the Ogre2 log, so this is
not merely a GLX reporting artifact. An earlier log reported D3D12, which is why
the runner now binds renderer identity to each repetition instead of trusting a
pre-run probe. The sensor world still reaches real time after warm-up, so this
mismatch alone does not explain the reported RTF 0.30 in the full drone stack.
This is a different machine from the stated i7-4790 / GTX 1080 Ti auxiliary
system.

An imported three-run suite from the auxiliary i7-4790 / GTX 1080 Ti system
confirmed ROS 2 Jazzy, Gazebo Sim 8.15.0, `perf`, 15.5 GiB RAM, NVIDIA driver
582.53, and Ogre2 D3D12 hardware rendering. Its 180-second runs produced median
RTF 0.999979 with only 0.027% run-to-run spread, 0.420 median average CPU cores,
and a 316,166,144-byte peak RSS. The suite diagnostic correctly classifies this
reference workload as real-time capped, so it is compatibility and stability
evidence rather than evidence about the reported RTF 0.30 drone workload.

## Next implementation slice

1. Run the new stress GUI/headless pair on the auxiliary machine, then repeat
   B0/B1 on the actual drone world.
2. Record the real drone launch, sensor specifications, physics step, bridge,
   perception, controller, and SITL configuration for B2-B7.
3. Add quality-adapter hooks for obstacle-avoidance latency and accuracy gates.
4. Add native Gazebo Transport collection after the CLI contract stabilizes.
5. Only after profiling, select a compute kernel for a C++/Rust FFI spike.

## Verification

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --locked
```
