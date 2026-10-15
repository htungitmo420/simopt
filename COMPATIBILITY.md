# Compatibility evidence

Updated: 2026-10-15

This matrix separates successful builds, runtime compatibility, and meaningful
performance evidence. A build or smoke test does not imply that every Gazebo
workload is fast on that platform.

| Platform | Hardware | Software | Renderer evidence | Build | Runtime | Performance evidence |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 | Intel i7-4790, 8 logical CPUs, 15.5 GiB RAM, GTX 1080 Ti | Ubuntu 24.04 environment, ROS 2 Jazzy, Gazebo Sim 8.15.0, NVIDIA driver 582.53 | Ogre2 `D3D12 (NVIDIA GeForce GTX 1080 Ti)` in all loaded-drone runs | Pass | Loaded camera/GPU-LiDAR messages, native sink, process-tree cleanup, and repeated collection pass | Raw-sink matrix: default GUI median RTF 0.2231, 960x540 observer 0.2270, compact 640x360 observer 0.2325, headless 0.2453. Compact improves 4.21% over default; headless improves 5.50% over compact. Sensor isolation with the raw sink and the real drone stack remain pending. |
| WSL2 | AMD Ryzen 5 6600H, RTX 3050 Ti, 32 GiB RAM | ROS 2 Jazzy, Gazebo Sim 8.15.0 | Controlled reference runs reported Ogre2 `llvmpipe`; an earlier log reported D3D12 | Pass | Minimal and camera/GPU-LiDAR reference worlds pass | Light reference reached approximately RTF 1.0, but software rendering invalidates GPU-performance conclusions. |
| Ubuntu 24.04 GitHub Actions | Hosted x86_64 runner; no Gazebo GPU assigned | Rust 1.85 | Not tested | Rust format, Clippy, unit tests, offline doctor, and sample analysis pass | Gazebo runtime not tested | None |

## Interpretation rules

- Compare performance only on the same physical machine, workload fingerprint,
  renderer, warm-up, measurement duration, and repetition policy.
- A suite near RTF 1.0 with low resource use is usually limited by the configured
  real-time target. It cannot measure unused throughput headroom.
- Renderer identity in each run is stronger evidence than `nvidia-smi` or a
  capability probe collected before Gazebo starts.
- The i7-4790 / GTX 1080 Ti system still needs the real drone B0-B7 matrix before
  any C++-to-Rust kernel choice is justified.

The earlier imported stationary stress pair on the i7-4790 / GTX 1080 Ti stayed at
RTF 1.0 in GUI and headless modes. It used the previous runner, which released
sensor subscribers before measurement, and the GUI process metrics reported
zero CPU because descendant process groups were not tracked. Treat that archive
as hardware-renderer and runtime evidence only. The upgraded loaded-drone suite
supersedes it for GUI/headless performance evidence.

The first sensor-isolation matrix used `gz topic -e` as a persistent load
subscriber. It reliably identifies cameras as the dominant load, but the
LiDAR-only case accumulated about 6.9 GiB RSS and exposed text-serialization
backpressure in the harness. Absolute stress RTF and memory figures should be
refreshed with the raw Transport sink before further compatibility claims.

The raw-sink GUI matrix has now been repeated on the i7-4790 / GTX 1080 Ti.
All four cases used the same world fingerprint, timing policy, and D3D12
renderer. The compact observer reached median RTF 0.2325 versus 0.2231 for the
default GUI, a 4.21% improvement. Its maximum Gazebo process-tree RSS was about
890 MB versus 921 MB for the default. Headless reached 0.2453, only 5.50% above
compact, while using about 417 MB peak RSS. This supersedes the earlier 22.73%
GUI/headless estimate, which used the text subscriber and default GUI only.
