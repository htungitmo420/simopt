# Compatibility evidence

Updated: 2026-10-14

This matrix separates successful builds, runtime compatibility, and meaningful
performance evidence. A build or smoke test does not imply that every Gazebo
workload is fast on that platform.

| Platform | Hardware | Software | Renderer evidence | Build | Runtime | Performance evidence |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 | Intel i7-4790, 8 logical CPUs, 15.5 GiB RAM, GTX 1080 Ti | Ubuntu 24.04 environment, ROS 2 Jazzy, Gazebo Sim 8.15.0, NVIDIA driver 582.53 | Ogre2 `D3D12 (NVIDIA GeForce GTX 1080 Ti)` in all three imported runs | Pass | Camera and GPU LiDAR messages, process cleanup, and 180-second collection pass | Light reference suite is real-time capped: median RTF 0.999979, p05 0.999754, 0.420 median average CPU cores, 316,166,144-byte peak RSS. This is stability evidence, not drone-load evidence. |
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
