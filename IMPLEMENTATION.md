# Implementation status

Updated: 2026-10-15

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
- Optional Linux process-tree and process-group monitoring records CPU time,
  average CPU-core use, peak RSS, and peak process count during each measured
  interval, including Gazebo workers that create a new process group.
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
- `simopt suite-matrix` summarizes an ordered B2-B7 or sensor-isolation run,
  including wall cost per simulated second and provenance consistency checks.
- Repository scenarios and a runner script now isolate physics/IMU, the front
  camera, both cameras, GPU LiDAR, and the complete sensor set headlessly.
- A raw Gazebo Transport sink activates lazy sensors without converting image
  and laser protobuf payloads to terminal text. Scenario manifests can select a
  custom persistent subscriber command with `{topic}` substitution.
- A GUI matrix compares the default interface, observer-only 960x540 and
  compact 640x360 layouts, and a headless reference without changing sensors.
- A moving-drone reference world and paired GUI/headless scenarios exercise two
  cameras, a high-density GPU LiDAR, IMU, shadows, physics, and obstacle geometry.
  Persistent load-topic subscribers keep lazy rendering active during measurement.

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

A second imported GUI/headless stress pair was also capped near RTF 1.0. The
old stress world was stationary and its sensor-check subscribers exited before
measurement, so it did not maintain camera or LiDAR load. GUI CPU/RSS metrics
were also incomplete because the older collector followed only one process
group. These results remain renderer/runtime evidence but cannot establish a
GUI-versus-headless speedup. The runner now keeps declared `load_topics`
subscribed and follows descendant processes across process groups.

The upgraded loaded-drone suite from the auxiliary i7-4790 / GTX 1080 Ti is no
longer capped. Three headless runs measured median RTF 0.2796 and p05 0.2794;
three GUI runs measured median RTF 0.2278 and p05 0.2204. Workload, renderer,
and measurement policy match, so headless improves median RTF by 22.73%.
Headless used about 1.07 measured Gazebo CPU cores and 396 MiB peak RSS, versus
1.93 cores and 884 MiB for the GUI process tree. Both used the D3D12 GTX 1080 Ti
renderer. Sensor cost still needs B2-B5 isolation before changing fidelity.

The first B2-B5 matrix showed physics/IMU at RTF 0.9997, front camera at
0.4229, two cameras at 0.3282, GPU LiDAR alone at 0.9983, and the full sensor
set at 0.2841. This identifies cameras as the dominant load in this synthetic
world. The LiDAR-only case also reached about 6.9 GiB RSS because the original
`gz topic -e` load subscriber formatted high-rate laser messages as text faster
than it could drain them. The raw Transport sink removes that harness artifact;
the sensor and GUI matrices must be repeated with the new sink before selecting
a fidelity change. On the development WSL2 host, the raw sink increased the
five-second full-sensor smoke RTF from about 0.156 to 0.194.
The observer-only GUI, custom subscriber, configuration fingerprint, and
process-tree cleanup also passed together in a five-second integration run.

The upgraded drone stress smoke run completed locally with all camera, LiDAR,
and IMU streams active. On the development WSL2 software renderer it measured
RTF 0.1560, 7.19 average CPU cores, and about 779 MiB peak RSS. This proves the
new workload exposes a bottleneck; software rendering and the five-second
single repetition make it unsuitable for cross-host performance claims.
The matching GUI smoke reached RTF 0.1206 and the corrected sampler observed
three Gazebo processes, 7.64 average CPU cores, and about 1.38 GiB peak RSS.
That run verifies descendant-process accounting; it is not a performance
comparison because each smoke profile contains only one short repetition.

## Next implementation slice

1. Repeat the B2-B5 matrix with the raw Transport sink on the auxiliary machine.
2. Run the default/observer/compact/headless GUI matrix and select an interactive layout.
3. Repeat B0/B1 and the dominant camera cases on the actual drone world.
4. Record the real drone launch, sensor specifications, physics step, bridge,
   perception, controller, and SITL configuration for B2-B7.
5. Add quality-adapter hooks for obstacle-avoidance latency and accuracy gates.
6. Add native Gazebo Transport statistics collection after the CLI contract stabilizes.
7. Only after profiling, select a compute kernel for a C++/Rust FFI spike.

## Verification

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --locked
```
