# Plan for Optimizing Gazebo / ROS 2 Jazzy Simulations with Rust

Created: 2026-10-12. Status: in progress. The P0/P1 MVP now includes the portable core, CLI, capability diagnostics, Gazebo statistics collector, repeated benchmark runner, schemas, tests, and WSL2 smoke tests. The real drone workload has not been benchmarked yet. See [IMPLEMENTATION.md](IMPLEMENTATION.md).

## 1. Context and objectives

- Target simulation computer: Intel i7-4790, NVIDIA GTX 1080 Ti, running WSL2.
- Current workload: one drone with a camera and LiDAR for obstacle avoidance. The existing Gazebo integration is C++, and the reported Real Time Factor (RTF) is approximately 0.30.
- Still unknown for the target computer: Ubuntu version, RAM, exact Gazebo packages, renderer, physics engine, sensor settings, and whether PX4 or ArduPilot SITL is involved.
- This repository contains the portable Rust core, CLI, benchmark runner, and smoke worlds. It does not yet contain the drone's C++ source, world, launch files, or sensor configuration.

The objective is to build an **open-source** project that can diagnose, benchmark, and manage Gazebo + ROS 2 Jazzy configurations across different hardware. Rust should only replace or accelerate C++ computation after profiling identifies a worthwhile bottleneck. The i7-4790 + GTX 1080 Ti system is the first target machine, not a hardware requirement. Reuse comes from portable tooling, schemas, adapters, and measurement procedures; each robot and world still needs its own configuration and quality constraints. No single Rust module can guarantee a higher RTF for every simulation.

RTF is simulated time advanced divided by real elapsed time. An RTF of 0.30 means that one simulated second takes about 3.33 real seconds. The desired target is close to 1.0 for the complete workload, but feasibility can only be established after profiling. Reports must include relative improvement and simulation quality, not only average RTF.

## 2. Principles and scope

1. Measure before selecting code to rewrite in Rust. Replacing C++ with Rust is not evidence of a speedup.
2. Keep the existing Gazebo, physics, rendering, and ROS integration unless measurements justify a change. Rust does not automatically move Gazebo physics to the GPU.
3. Investigate WSL/GPU setup, Gazebo configuration, and C++ costs before porting algorithms.
4. Separate same-quality optimization from reduced fidelity. Any lower sensor rate, resolution, ray count, or physics frequency must be explicit in the report.
5. Apply configuration per run, retain a baseline, and support rollback. Do not silently modify drivers, the original world, or global WSL configuration.
6. The first versions select explicit profiles and run controlled benchmarks. They do not adjust fidelity automatically during flight.
7. The core must not depend on NVIDIA, WSL, or a specific CPU model. Platform and acceleration features are optional adapters; the portable functionality must continue to work when an adapter is unavailable.

Example of the Amdahl limit: if a custom kernel occupies 20% of the critical path, making it twice as fast gives an ideal speedup of only `1 / (0.8 + 0.2 / 2) ≈ 1.11`, increasing RTF from 0.30 to roughly 0.33. Removing the kernel cost entirely would only reach about 0.375. This is an illustration, not a measurement from the target computer. CPU sample percentages alone do not establish the critical-path fraction.

## 3. Platform checks: highest priority

### 3.1 Confirm the software pairing

ROS 2 Jazzy's default Gazebo pairing is **Harmonic**. A description such as “latest Gazebo” is insufficient; record the actual package version and installation source, and avoid mixing incompatible ABIs. Non-default pairings require separate compatibility testing. See the [Gazebo documentation for installing Gazebo with ROS](https://gazebosim.org/docs/latest/ros_installation/).

Collect the following on the target computer:

```powershell
# Windows PowerShell
wsl --version
wsl --status
wsl -l -v
```

```bash
# Ubuntu inside WSL2; source the actual workspace when applicable
source /opt/ros/jazzy/setup.bash
cat /etc/os-release
uname -a
printenv ROS_DISTRO RMW_IMPLEMENTATION
gz sim --versions
gz sim --help
dpkg-query -W 'ros-jazzy-ros-gz*' 'ros-jazzy-gz-*' 'gz-*'
lscpu
free -h
```

Record missing commands as unavailable rather than assuming the environment is valid. Save Rust/Cargo and C++ compiler versions, launch arguments, expanded SDF/URDF, mesh versions, bridge configuration, controller versions, and SITL versions when present.

### 3.2 Verify the renderer and GPU on WSL2

WSLg can expose OpenGL acceleration through a virtual GPU when the Windows driver and Linux graphics stack are compatible. `nvidia-smi` working does not prove that Gazebo camera rendering uses the NVIDIA GPU. See [Microsoft's WSL GUI application documentation](https://learn.microsoft.com/en-us/windows/wsl/tutorials/gui-apps).

```bash
glxinfo -B
ls -l /dev/dxg
/usr/lib/wsl/lib/nvidia-smi
```

- Record the OpenGL vendor and renderer and inspect the Ogre2 log generated by Gazebo. EGL headless rendering may select a different device from GLX.
- If the renderer contains `llvmpipe` or `softpipe`, investigate software rendering first. A working WSLg D3D12 renderer often includes `D3D12` and the GPU name; the vendor string does not have to say NVIDIA.
- Compare Windows Task Manager GPU engines, CPU load, and memory while the actual workload runs. An unsupported WSL metric must be reported as `unavailable`, not as zero.
- Use a compatible NVIDIA Windows driver. Do not install a Linux NVIDIA display driver inside WSL. `nvidia-smi` has feature limitations under WSL. See the [NVIDIA CUDA on WSL guide](https://docs.nvidia.com/cuda/wsl-user-guide/index.html).
- Compare a workspace and asset cache stored in the Linux filesystem, such as `~/sim_ws`, with `/mnt/c` or `/mnt/e` when mounted-drive I/O may matter. Measure the difference rather than assuming it is the cause.
- Record CPU and RAM assigned to WSL, swap, CPU affinity or quota, and concurrent Windows load. Change `.wslconfig` only after measurements show a resource limit and the host's total memory is known. See [Microsoft's WSL configuration reference](https://learn.microsoft.com/en-us/windows/wsl/wsl-config).

The current development machine exposes `/dev/dxg`, WSL D3D12 libraries, and an RTX 3050 Ti through `nvidia-smi`. Controlled reference runs currently report `llvmpipe`; an earlier Ogre2 log reported D3D12, showing that a machine-level GPU probe or stale renderer log is insufficient. The benchmark therefore records a fresh renderer identity for every run. The sensor reference world still approaches real time after warm-up, so the software-renderer issue must be fixed but does not by itself explain the reported drone RTF of 0.30.

### 3.3 Compare display modes

Measure the full GUI, server-only mode, and EGL headless rendering when supported. Gazebo supports `--headless-rendering` with Ogre2; validate both the backend and sensor output before selecting it on WSL2. Server-only mode may still require rendering for cameras and GPU LiDAR. See [Gazebo Harmonic headless rendering](https://gazebosim.org/api/sim/8/headless_rendering.html).

```bash
gz sim -s -r /path/to/world.sdf
gz sim -s -r --headless-rendering /path/to/world.sdf
```

If EGL fails or falls back to an unsuitable backend, retain the mode that works correctly. Verify that required sensor topics still publish. Remove SceneBroadcaster or GUI-related systems only after proving that no required component depends on them. If WSL remains a suspected bottleneck, compare the same world on native Ubuntu on equivalent hardware when possible.

## 4. Establish the baseline and locate bottlenecks

### 4.1 Repeatable measurement procedure

- Fix the scenario, random seed, initial state, physics step, sensors, subscribers, controller, and drone speed. Preload assets and exclude model downloads and shader compilation from the measured interval.
- Start with a 30-second real-time warm-up, a 180-second real-time measurement, and at least three repetitions. Alternate baseline and candidate order to reduce drift. These values remain configurable.
- Use a fixed wall duration for performance measurement. Use a fixed mission or simulated duration for behavior validation. They answer different questions.
- Compute total RTF as `Δsim_time / Δmonotonic_wall_time`. Report median and p05 for one-second RTF windows, plus variation across repetitions.
- Exclude startup, pause, and reset intervals. Start a new segment when simulation time moves backward. Do not confuse Gazebo WorldStatistics real time with an external monotonic clock.
- Collect CPU by process and core, RSS and swap, available GPU metrics, physics and callback durations, p50/p95/p99 latency, queue depth, dropped frames, and sensor-to-command latency.
- Report sensor frequency in both simulation time and wall time. A 30 Hz simulated sensor produces only about 9 Hz in wall time when RTF is 0.30.
- Measure collector overhead. Run deep profilers separately. `ros2 topic hz`, `ros2 topic bw`, RViz, rosbag, and CLI subscribers add work and may activate lazy sensors, so record them in the scenario.

Discover the real stats topic with `gz topic -l`, then subscribe to `/world/<world_name>/stats`. The current MVP reads `gz.msgs.WorldStatistics` through `gz topic --json-output` and timestamps arrivals with its own monotonic clock. A future native Gazebo Transport adapter can remove CLI serialization overhead after the interface is stable.

### 4.2 Cost-isolation matrix

| Case | Configuration | Question answered |
| --- | --- | --- |
| B0 | Existing stack, GUI, and subscribers | What is the user's current baseline? |
| B1 | B0 without GUI/RViz | How much does display cost? |
| B2 | World + drone + physics; expensive sensing, perception, and bridges disabled | Is physics/contact processing already slow? |
| B3 | B2 + camera + minimal consumer | What do camera rendering and readback cost? |
| B4 | B2 + LiDAR + minimal consumer | What does the actual LiDAR implementation cost? |
| B5 | B2 + both sensors + minimal consumers | What synchronization or interaction cost appears? |
| B6 | B5 + bridge + minimal ROS consumers | What do conversion, copy, DDS, and QoS cost? |
| B7 | B6 + real perception, planner, controller, and SITL | What is the full-system cost and behavior? |

Use a repeatable command trajectory or a stationary diagnostic case for B2–B6. Disabling sensing must not unintentionally change controller behavior. Keep a minimal consumer connected when a sensor is lazy. Component-removal cases are diagnostic and are not product optimization results. Differences between adjacent cases are indicators, not an exact additive cost decomposition.

Use `perf` when supported by the WSL kernel and permissions. Otherwise use timers in plugins, available sampling tools, and `ros2_tracing` for ROS callbacks. Measure wall duration for major phases and avoid adding durations from phases that execute concurrently. See the official [ROS 2 Jazzy tracing tutorial](https://github.com/ros2/ros2_documentation/blob/jazzy/source/Tutorials/Advanced/ROS2-Tracing-Trace-and-Analyze.rst).

## 5. Optimize configuration and C++ before porting

| Measured bottleneck | First experiments | Acceptance condition |
| --- | --- | --- |
| Software rendering or rendering setup | Fix WSL backend, remove GUI, test compatible headless mode, reduce unnecessary visual effects | Camera/LiDAR data remains correct and sufficient |
| Camera | Sweep resolution and update rate; reduce shadows or visual detail when allowed | Required obstacles remain detectable |
| LiDAR | Confirm CPU ray or GPU LiDAR; sweep samples, channels, rate, and range | Small and safety-critical obstacles remain observable |
| Physics and contacts | Use primitive or convex collision geometry, remove unnecessary self-collision, validate inertias and joints | Collision shape and dynamics remain within limits |
| Bridge and DDS | Bridge only required topics and directions, use bounded queues and appropriate QoS | No unbounded backlog or QoS mismatch |
| C++ plugin | Release build, reused buffers, valid entity caching, fewer copies, logs, and locks | Behavior remains correct and measurements improve |
| Thread contention | Sweep worker counts and avoid independent oversized pools | Tail latency and RTF both meet targets |

Implementation details:

- Benchmark optimized C++ and Rust release builds with equivalent algorithms and data.
- Entity or component caches must handle spawn, despawn, and reset. Do not retain pointers after their lifetime ends.
- Lowering downstream ROS publication frequency does not necessarily lower camera render frequency. Moving LiDAR to the GPU is an experiment, not an assumed improvement under WSL.
- A lazy bridge can reduce bridge work without guaranteeing that every upstream sensor stops computing. Keep `/clock` available and verify YAML/QoS support in the installed `ros_gz_bridge`. See the [Jazzy ros_gz_bridge documentation](https://github.com/gazebosim/ros_gz/blob/jazzy/ros_gz_bridge/README.md).
- Use SensorDataQoS or best effort only where dropped samples are acceptable, with a small bounded queue. Do not apply it indiscriminately to commands, services, `/clock`, or TF. See the official [ROS 2 Jazzy QoS documentation](https://github.com/ros2/ros2_documentation/blob/jazzy/source/Concepts/Intermediate/About-Quality-of-Service-Settings.rst).
- Increasing `max_step_size` reduces temporal resolution. Sweep it under explicit flight-control, motor-model, contact, and tunneling checks. Setting `<real_time_factor>1.0</real_time_factor>` specifies a target; it does not create compute capacity. See [Gazebo Harmonic SDF worlds](https://gazebosim.org/docs/harmonic/sdf_worlds/).
- Test only solver and engine options supported by the installed physics engine. Do not copy Gazebo Classic ODE settings into Harmonic without validation.
- Some Harmonic versions support `parallel_postupdates`; benchmark both states when the installed version provides it, because synchronization can cost more than small plugin work. See [Gazebo server configuration](https://gazebosim.org/api/sim/8/server_config.html).

## 6. Reusable Rust architecture

### 6.1 Two independent layers

**Portable tooling: `simopt-cli` + `simopt-core`.** This layer runs outside the physics loop. It captures capabilities and metrics, launches repeated benchmarks, compares profiles, and writes reports. Existing robots do not need to be rewritten to use it. The tooling finds and verifies optimizations; the collector itself does not make Gazebo faster.

**Optional acceleration: `simopt-kernels` + a C++ adapter.** Add this layer only after profiling finds a significant kernel, such as point-cloud filtering, occupancy updates, distance computation, or trajectory scoring. Do not choose a kernel before evidence exists.

```text
Outside the simulation loop:
  simopt-cli -> process / launch runner -> Gazebo + ROS 2
       |                                  |
       +<- stats and OS metrics adapters -+
       +-> JSON / CSV reports

Inside a plugin selected for optimization:
  C++ Gazebo / ROS adapter -> batched FFI -> Rust kernel
         <- timestamped / sequenced output <-+
```

Keep plugin registration and EntityComponentManager access in C++. Pass only the required data into Rust. Gazebo systems normally read post-physics state in PostUpdate and apply control before the next step in PreUpdate. See [Gazebo Harmonic system plugins](https://gazebosim.org/api/sim/8/createsystemplugins.html).

### 6.2 FFI and runtime requirements

- Prefer a narrow versioned C ABI and a Rust `staticlib` for the first spike. Link it into a position-independent C++ shared plugin. Expose `create`, `process_batch`, `reset`, and `destroy` around an opaque handle, slices, output buffers, and status codes.
- Document allocation and ownership, borrow duration, alignment, maximum sizes, and thread rights. Do not pass Gazebo objects, STL containers, or Protobuf objects through the C ABI. Never retain borrowed pointers after the call returns.
- Batch work, reuse buffers, and measure copy and marshaling cost. Zero-copy is a conditional goal, not a promise.
- Prevent Rust panics and C++ exceptions from crossing the boundary. Return explicit status values for expected errors. If CXX is selected instead, follow its Result/exception rules and do not assume a panic can be recovered. See [CXX error handling](https://cxx.rs/binding/result.html).
- A synchronous kernel on the control path must finish within a measured budget. Asynchronous workers may only access owned data, never the EntityComponentManager. Do not consume stale results without an algorithm-specific check.
- Use bounded queues with an explicit drop or backpressure policy. Commands require sequence, timestamp, and expiry metadata, plus defined stale-data behavior.
- A pause must not integrate state. Reset or time jumps create a new epoch, clear queues, and invalidate older results. Join workers before destroying the handle or unloading the plugin.
- Sweep worker counts such as 1, 2, and 4. Do not automatically use every logical CPU or add Tokio/Rayon to each simulation tick without evidence.
- Integrate Cargo with ament_cmake/colcon using an explicit dependency graph and generated headers. Pin toolchains and dependencies. See [CXX CMake integration](https://cxx.rs/build/cmake.html) and [Cargo build profiles](https://doc.rust-lang.org/cargo/reference/profiles.html).
- Keep `target-cpu=native` optional for a machine-specific build. Distributed binaries must retain the advertised CPU baseline. Do not change floating-point precision or enable broad fast-math settings without validation.
- Keep ROS in the C++ adapter for the first kernel spike. Evaluate `rclrs`, message generation, Jazzy QoS, and colcon integration separately if a standalone Rust node later becomes useful. See [ros2_rust](https://github.com/ros2-rust/ros2_rust).

### 6.3 Scenario and report contracts

The MVP implements:

- `simopt doctor` for capability detection.
- `simopt collect` for raw monotonic wall/simulation timestamps.
- `simopt analyze` for one-run RTF reports.
- `simopt compare` for a baseline/candidate performance gate.
- `simopt bench --scenario ...` for launch, readiness checks, warm-up, repeated collection, cleanup, and aggregation.

The versioned JSON scenario stores a label, launch executable and argument array, environment overrides, working directory, stats topic, required topics, Gazebo command, warm-up, wall duration, repetitions, startup timeout, shutdown grace period, delay between runs, and RTF window size. Commands are never passed through a shell.

Future manifest fields should capture software and hardware fingerprints, world and asset hashes, random seed, sensor inventory, bridge directions and QoS, physics engine and step, controller/SITL rates, quality constraints, and metric availability.

Do not modify XML worlds with regular expressions. Use a namespace-aware parser and validate resolved SDF, includes, URDF, and Xacro. A profile must generate a derived configuration, record every difference, and retain the baseline. The runner must only terminate process groups that it created.

## 7. Quality gates for obstacle-avoidance drones

An RTF improvement is accepted only when mission behavior and sensor data remain within defined limits. Maintain separate `baseline`, `balanced`, and `throughput` profiles. Every profile must state its fidelity changes; use `throughput` only for tasks that allow lower fidelity.

- Use ROS simulation time consistently with one authoritative `/clock`. For PX4 or ArduPilot, validate lockstep and wall-clock timeout behavior rather than assuming ROS parameters are sufficient.
- Define maximum drone speed, minimum obstacle size, minimum clearance, allowable pose/velocity/attitude error, collision rate, and p99 latency before sweeping configuration. These values have not yet been provided.
- Derive minimum sensor rate from travel per sample `v_max / f_sensor`, perception/control delay, and braking ability. A useful safety relation is `d_margin >= v_max × T_delay + d_brake + d_uncertainty`; the braking model must fit the drone.
- Separate simulation-time and wall-time latency. Incorrect headers, timestamps, or stale data can improve apparent RTF while breaking avoidance behavior.
- Include hover, straight flight, small or nearby obstacles, narrow corridors, moving obstacles, rapid turns, high sensor load, pause/reset, and controlled sample loss.
- Compare sensor frames and calibration, trajectories, clearance, collision count, mission success, and command age. Use numeric tolerances rather than bitwise equality across physics engines or thread schedules.
- For a final configuration, run multiple seeds. Start with at least ten episodes for each main scenario and report variation; ten episodes are still insufficient to estimate rare collision probability precisely.

## 8. Roadmap and phase gates

| Phase | Work and deliverables | Completion condition |
| --- | --- | --- |
| P0 — Platform | Capability manifest, version/WSL/GPU checks, initial baseline | Actual renderer and repeatable baseline are known |
| P1 — Measurement | Rust runner and collector, B0–B7 matrix, bottleneck report | Critical-path evidence and collector overhead are known |
| P2 — Existing-stack optimization | Gazebo/ROS/C++ profiles with quality-preserving and reduced-fidelity comparisons | Beneficial changes retained; need for Rust kernel established |
| P3 — Rust spike | One kernel and equivalent optimized C++/Rust microbenchmarks including FFI/copy | Amdahl and latency budgets justify integration; otherwise stop the port |
| P4 — Integration | C++ adapter, lifecycle/reset/error handling, configurable C++ fallback | Correct load/unload, bounded queues, functional tests pass |
| P5 — Full-system validation | Repeated benchmarks, drone episodes, at least 30 minutes real-time soak | Repeatable improvement, stable RSS/queues, mission gates pass |
| P6 — Open source release | CLI/schema/contribution docs, another example world, CI, compatibility matrix | Portable core is demonstrated on multiple machines with honest support labels |

Suggested acceptance threshold for a Rust change: median full-system RTF improves by at least 10% relative to already optimized C++, exceeds run-to-run noise, and keeps p05 RTF, latency, and quality inside their limits. This is a decision threshold, not a promised speedup. If the complete workload cannot reach RTF 1.0, report the remaining cost and environmental or hardware limits.

## 9. Repository layout and backlog

```text
crates/simopt-core/        # portable schemas, metrics, aggregation, comparison
crates/simopt-cli/         # doctor, collector, runner, reports
crates/simopt-kernels/     # create only after P3 is justified
ros_ws/src/simopt_adapter/ # future Gazebo Transport collector and C++ FFI plugin
configs/scenarios/         # versioned benchmark scenarios
configs/profiles/          # future baseline, balanced, throughput profiles
benchmarks/                # reproducible workloads and harnesses
reports/                   # local manifests, raw metrics, comparisons
docs/                      # WSL setup, ABI, integration, rollback
examples/                  # reusable minimal worlds and launch examples
schemas/                   # versioned JSON contracts
.github/workflows/         # CI checks
```

- [x] Capture the target i7-4790 / GTX 1080 Ti machine's Ubuntu, RAM, driver, renderer, and package versions.
- [ ] Record the real drone launch command, camera/LiDAR types, resolution, samples, update rates, physics step, and flight controller or SITL.
- [ ] Define obstacle-avoidance accuracy and latency limits.
- [ ] Run B0 and B1 on the target machine and verify its actual Ogre2 renderer.
- [x] Implement a minimal Rust collector for Gazebo WorldStatistics JSON.
- [x] Validate collection on Gazebo Sim 8.15.0 / Harmonic in WSL2.
- [x] Implement a repeated benchmark runner with warm-up, readiness checks, logs, process-group cleanup, per-run reports, and suite aggregation.
- [x] Add Linux process-tree CPU time, average CPU-core use, peak RSS, and peak process count to each run, with configurable sampling.
- [x] Add a versioned environment and workload fingerprint to the suite report.
- [x] Add a repository-owned camera/GPU-LiDAR world with per-run renderer identity and pre-measurement sensor-message readiness checks.
- [x] Keep declared sensor topics subscribed during measurement so lazy camera and GPU LiDAR work is actually included.
- [x] Add a synthetic moving-drone stress world with two cameras, dense GPU LiDAR, IMU, shadows, collisions, and local obstacle assets.
- [ ] Complete the B0–B7 matrix with the real camera/LiDAR drone workload.
- [ ] Optimize Gazebo/ROS/C++ with evidence and retain rollback data.
- [ ] Select one Rust kernel only when profiling establishes value.
- [ ] Validate drone behavior, resource stability, and integration on WSL2.
- [x] Run the tooling on another world and at least one additional computer.
- [ ] Add a native Gazebo Transport metrics adapter after the CLI contract stabilizes.
- [x] Publish a compatibility matrix that distinguishes build, runtime, and performance evidence.

## 10. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Low RTF caused by WSL or renderer fallback | Verify the renderer and benchmark display modes before designing a Rust kernel |
| Ported kernel is a small part of physics cost | Apply Amdahl limits and focus on world/configuration or platform evidence |
| FFI, copy, or thread overhead removes the gain | Batch work, reuse buffers, and compare the complete pipeline with optimized C++ |
| Lower sensor fidelity misses obstacles | Enforce mission quality gates and publish fidelity changes per profile |
| Larger physics step breaks control or collision | Define control budgets and validate dynamics/contact before accepting it |
| Version or ABI differences across machines | Pin manifests, build against the installed Gazebo, and run compatibility tests |
| Asynchronous results survive a reset | Use epochs, sequence numbers, timestamps, and explicit invalidation |
| GPU or CPU metrics are unavailable in WSL | Report `unavailable`; use renderer logs, timers, and complementary Windows metrics |
| Benchmark runner leaves child processes | Use a dedicated process group, graceful termination, a bounded grace period, then forced cleanup |
| Benchmark noise creates false improvements | Warm up, repeat, alternate order, report distributions, and require a gain above noise |

Implementation priority remains: verify WSL/GPU and versions → establish baseline and profile → optimize Gazebo/ROS/C++ → add an evidence-backed Rust kernel → validate drone behavior and portability.

## 11. Open-source design across different hardware

### 11.1 Meaning of support

Publish three separate support levels:

1. **Build:** the advertised core, CLI, or adapter compiles and links for a target.
2. **Runtime:** Gazebo/ROS launches, metrics are collected, and optional features have clear fallback behavior.
3. **Performance:** repeated evidence shows an improvement for a specific machine and workload while quality gates pass.

A successful build does not prove that the GPU is active or RTF improves. The core, CLI, collector, and repeated runner have a WSL2 runtime smoke pass; no multi-machine performance claim has been established.

The MVP reference environment is Ubuntu 24.04 + ROS 2 Jazzy + Gazebo Harmonic on native Linux x86_64, with WSL2 x86_64 as a secondary runtime. Ubuntu 24.04 is the supported Jazzy deb platform; ROS documentation lists amd64 and arm64. See the official [ROS 2 Jazzy Ubuntu installation guide](https://github.com/ros2/ros2_documentation/blob/jazzy/source/Installation/Ubuntu-Install-Debs.rst).

ARM64 is a later target after dependency, renderer, and runtime testing on real hardware. A Rust target existing does not prove that the complete ROS/Gazebo stack works on every ARM board. Windows-native and macOS Gazebo runtime support are outside the MVP; they may still build and test the portable analysis core.

### 11.2 Portable core and capability adapters

| Component | Responsibility | Hardware or platform dependency |
| --- | --- | --- |
| `simopt-core` | RTF analysis, aggregation, comparison, stable data contracts | No Gazebo, ROS, WSL, or NVIDIA APIs |
| `simopt-cli` runner | Launch orchestration, logs, timeout, cleanup | OS process backend; Linux/WSL first |
| Gazebo/ROS adapter | Stats, clock, topics, configuration, future FFI plugin | Tested Gazebo/ROS versions and ABI |
| System metrics adapter | CPU, RAM, threads, cgroup or quota limits | Linux-native or WSL capability probes |
| GPU metrics adapter | Device, renderer, and available utilization data | Optional NVIDIA/AMD/Intel-specific adapters |
| `simopt-kernels` | Algorithms behind a stable contract | Scalar CPU baseline; optional SIMD/GPU backends |
| Scenario adapter | Launch, robot, sensors, mission quality | Workload-specific; the drone is one example |

`doctor` must distinguish “GPU detected,” “renderer active,” “GPU metrics available,” and “GPU kernel works.” A single GPU boolean is insufficient. The current implementation detects platform kind, CPU features, memory, ROS/Gazebo tools, GLX, NVIDIA, `perf`, and the latest Gazebo Ogre2 renderer log.

CPU execution is the default and CUDA is not required to build the core or CLI. A CPU fallback for a custom kernel does not replace the GPU required by a Gazebo camera. Platform adapters are selected through capabilities; native Linux must not execute WSL-specific commands, and AMD/Intel systems must not fail merely because `nvidia-smi` is absent.

Distributed binaries must use the advertised CPU baseline. SIMD belongs in isolated functions selected after runtime feature detection, with an equivalent scalar fallback. On x86, Rust provides `is_x86_feature_detected!`; ARM requires its own tested mechanism. See [Rust runtime CPU feature detection](https://doc.rust-lang.org/std/macro.is_x86_feature_detected.html).

### 11.3 Profiles selected from evidence

Do not publish presets such as “i7 means four workers” or “powerful GPU means GPU LiDAR is faster.” Profile selection uses the capability report, workload, measured bottleneck, RTF and latency targets, and user-provided quality limits.

- v0.1: the user selects an explicit profile; the tool reports every change, condition, and baseline/candidate result.
- v0.2: bounded offline sweeps for worker count, queue sizes, render mode, and allowed parameters. Every sweep has a run budget and retains raw data.
- Sweep fidelity only when the scenario declares an allowed domain. Preserve baseline fidelity when quality constraints are missing.
- Reject candidates that fail quality gates or reproducibility. Select among remaining candidates using RTF and latency. Keep the baseline when there is no clear benefit.
- Confirm a selected profile in a fresh run and additional seeds. Bind results to a hardware, software, and workload fingerprint and invalidate them when that fingerprint changes.
- Documentation should include CPU-bound, rendering-bound, and bridge/copy-bound examples so users understand why a profile was selected.

### 11.4 Test matrix and community benchmarks

| Target group | Required checks | Current status |
| --- | --- | --- |
| x86_64 Linux, CPU core | Build core/CLI, unit/contract tests, minimal physics world | Build tested in Ubuntu 24.04 WSL2; native Linux pending |
| x86_64 Linux + GPU rendering | Camera/LiDAR, renderer identity, optional metrics, quality/performance | Pending |
| WSL2, current development machine | Core/CLI, stats collector, repeated runner, sensor smoke | Runtime smoke pass; Ogre2 uses llvmpipe |
| WSL2, i7-4790 + GTX 1080 Ti target | Actual renderer and drone workload | Pending |
| AMD/Intel GPU or no discrete GPU | Adapter fallback and supported sensing without NVIDIA dependency | Pending |
| ARM64 Linux | Native build, link, runtime, scalar/SIMD equivalence | After MVP |
| Container | CPU/RAM quota detection, dependencies, renderer passthrough | After MVP |

CI covers format, lint, unit tests, schemas and parsers, the portable analyzer, and capability detection with external probes disabled. Gazebo integration and performance require a dedicated runner or a community report with a recorded environment. Cross-compilation counts only as a build pass. Absolute RTF must not gate pull requests on shared runners with variable load.

Before calling the first release portable across machines, require runtime tests on at least two physical x86_64 computers with different CPUs, including native Linux and WSL2, plus one case where the core works without an NVIDIA adapter. This gate does not require camera operation without a GPU. AMD/Intel GPU and ARM support must remain explicitly unverified until separate evidence exists.

The reference workload set should include a minimal physics world, a camera/LiDAR world, and an obstacle-avoidance drone demonstration. Keep the drone quality gate separate from the core so wheeled robots and manipulators can define their own adapters. Community reports should contain CPU/GPU model, assigned resources, software versions, renderer, workload hash, baseline/candidate profiles, repetitions, fidelity, and unavailable metrics. Compare speedups only on the same machine and workload.

### 11.5 Packaging and user experience

- Provide a clean Ubuntu quick start: clone → dependency check → build → `doctor` → run the example world → read the report. Accept paths through CLI arguments or environment configuration; never hardcode drive letters or usernames.
- Publish source first. Add CLI binaries and ROS adapter packages only after their target and ABI boundaries are stable. Publish checksums and dependency versions.
- State the minimum supported Rust version, schema and ABI versions, changelog, and compatibility policy. Reject unsupported contracts with actionable errors.
- Keep the MIT license and audit third-party dependency and example-asset licenses before release.
- Maintain README, CONTRIBUTING, performance issue templates, adapter examples, and backend extension documentation. Performance pull requests require reproducible baseline/candidate evidence and quality checks.
- Store reports locally by default. Sharing is an explicit action. Community exports should be able to omit hostnames, usernames, and personal paths; never upload logs or worlds automatically.

The first open-source value proposition is straightforward: users within the supported scope can identify bottlenecks and verify a configuration on their own machines. Rust kernels remain conditional acceleration features added only when workload evidence justifies them.
