# Contributing

Thank you for helping make Gazebo performance work reproducible across hardware.

## Development checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Keep `simopt-core` free of ROS, Gazebo, operating-system, and GPU dependencies.
Platform integrations belong in adapters and must report missing capabilities
instead of silently writing zero values.

## Performance changes

A performance pull request should include:

- hardware and allocated CPU/RAM details;
- OS/WSL/container, ROS, Gazebo, renderer, and driver versions;
- scenario/world hash and fidelity settings;
- raw baseline and candidate reports on the same machine and workload;
- warm-up, duration, repetition count, median and low-tail RTF;
- latency and task-specific quality results;
- any unavailable metrics.

Do not use an absolute RTF threshold in ordinary CI. Shared runners are too
variable for that result to be meaningful.

## Privacy

Reports stay local by default. Remove hostnames, usernames, and personal paths
before attaching reports to issues. Never upload private worlds or logs without
the owner's explicit permission.

