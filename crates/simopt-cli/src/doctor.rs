use serde::Serialize;
use simopt_core::RendererIdentity;
use std::{
    env, fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CAPABILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct CapabilityReport {
    schema_version: u32,
    operating_system: &'static str,
    architecture: &'static str,
    platform_kind: PlatformKind,
    cpu_model: Option<String>,
    logical_cpu_count: Option<usize>,
    memory_total_bytes: Option<u64>,
    cpu_features: Vec<&'static str>,
    probes: Vec<Probe>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PlatformKind {
    LinuxNative,
    Wsl,
    Container,
    WindowsNative,
    MacOs,
    Other,
}

#[derive(Debug, Serialize)]
struct Probe {
    name: &'static str,
    state: ProbeState,
    command: String,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProbeState {
    Available,
    Unavailable,
    Skipped,
    Failed,
    TimedOut,
}

pub fn collect(skip_probes: bool) -> CapabilityReport {
    let os_release = read_optional("/proc/sys/kernel/osrelease");
    let platform_kind = classify_platform(
        env::consts::OS,
        os_release.as_deref(),
        Path::new("/.dockerenv").exists()
            || read_optional("/proc/1/cgroup").is_some_and(|v| {
                let lower = v.to_ascii_lowercase();
                lower.contains("docker")
                    || lower.contains("containerd")
                    || lower.contains("kubepods")
            }),
    );

    let mut probes: Vec<Probe> = [
        ("ros2", &["--help"][..]),
        ("gz", &["sim", "--versions"][..]),
        ("glxinfo", &["-B"][..]),
        (
            "nvidia-smi",
            &["--query-gpu=name,driver_version", "--format=csv,noheader"][..],
        ),
        ("perf", &["--version"][..]),
    ]
    .into_iter()
    .map(|(program, arguments)| probe(program, arguments, skip_probes))
    .collect();
    probes.push(gazebo_renderer_log_probe(skip_probes));

    let mut notes = vec![
        "A detected GPU does not prove that Gazebo is rendering on that GPU.".into(),
        "Missing optional probes are reported as unavailable and do not disable RTF analysis."
            .into(),
    ];
    let glx_software = probes
        .iter()
        .any(|probe| probe.name == "glxinfo" && software_renderer_detected(probe));
    let gazebo_renderer = probes
        .iter()
        .find(|probe| probe.name == "gazebo-renderer-log");
    let gazebo_software = gazebo_renderer.is_some_and(software_renderer_detected);
    let gazebo_hardware = gazebo_renderer.is_some_and(hardware_renderer_detected);
    if gazebo_software {
        notes.push(
            "The latest Gazebo Ogre2 log reports software rendering (llvmpipe/softpipe); resolve renderer acceleration before attributing camera or GPU LiDAR cost to application code."
                .into(),
        );
    } else if glx_software && gazebo_hardware {
        notes.push(
            "GLX reports software rendering, while the latest Gazebo Ogre2 log reports a hardware renderer. Use the Gazebo log as the relevant evidence for the tested headless sensor workload."
                .into(),
        );
    } else if glx_software {
        notes.push(
            "GLX reports software rendering (llvmpipe/softpipe); run a Gazebo rendering sensor to verify the renderer used by the actual workload."
                .into(),
        );
    }
    if matches!(platform_kind, PlatformKind::WindowsNative) {
        notes.push(
            "Gazebo/ROS runtime support is intended for Linux or WSL2 in the MVP; run doctor inside that environment."
                .into(),
        );
    }

    CapabilityReport {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        operating_system: env::consts::OS,
        architecture: env::consts::ARCH,
        platform_kind,
        cpu_model: cpu_model(),
        logical_cpu_count: std::thread::available_parallelism().ok().map(usize::from),
        memory_total_bytes: linux_memory_total_bytes(),
        cpu_features: cpu_features(),
        probes,
        notes,
    }
}

fn classify_platform(os: &str, kernel_release: Option<&str>, in_container: bool) -> PlatformKind {
    if os == "linux" {
        if kernel_release.is_some_and(|value| value.to_ascii_lowercase().contains("microsoft")) {
            PlatformKind::Wsl
        } else if in_container {
            PlatformKind::Container
        } else {
            PlatformKind::LinuxNative
        }
    } else if os == "windows" {
        PlatformKind::WindowsNative
    } else if os == "macos" {
        PlatformKind::MacOs
    } else {
        PlatformKind::Other
    }
}

fn probe(program: &'static str, arguments: &[&str], skipped: bool) -> Probe {
    let command = std::iter::once(program)
        .chain(arguments.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    if skipped {
        return Probe {
            name: program,
            state: ProbeState::Skipped,
            command,
            detail: None,
        };
    }

    let mut child = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Probe {
                name: program,
                state: ProbeState::Unavailable,
                command,
                detail: Some("command not found in PATH".into()),
            };
        }
        Err(error) => {
            return Probe {
                name: program,
                state: ProbeState::Failed,
                command,
                detail: Some(error.to_string()),
            };
        }
    };

    let started = Instant::now();
    let output = loop {
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output(),
            Ok(None) if started.elapsed() >= Duration::from_secs(5) => {
                let _ = child.kill();
                let detail = match child.wait_with_output() {
                    Ok(output) => first_nonempty_line(&output.stderr)
                        .or_else(|| first_nonempty_line(&output.stdout))
                        .map(|line| format!("timed out after 5 seconds; {line}"))
                        .unwrap_or_else(|| "timed out after 5 seconds".into()),
                    Err(error) => format!("timed out after 5 seconds; {error}"),
                };
                return Probe {
                    name: program,
                    state: ProbeState::TimedOut,
                    command,
                    detail: Some(detail),
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe {
                    name: program,
                    state: ProbeState::Failed,
                    command,
                    detail: Some(error.to_string()),
                };
            }
        }
    };

    match output {
        Ok(output) if output.status.success() => Probe {
            name: program,
            state: ProbeState::Available,
            command,
            detail: probe_detail(program, &output.stdout, &output.stderr),
        },
        Ok(output) => Probe {
            name: program,
            state: ProbeState::Failed,
            command,
            detail: first_nonempty_line(&output.stderr)
                .or_else(|| first_nonempty_line(&output.stdout))
                .or_else(|| Some(format!("exit status {}", output.status))),
        },
        Err(error) => Probe {
            name: program,
            state: ProbeState::Failed,
            command,
            detail: Some(error.to_string()),
        },
    }
}

fn first_nonempty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(500).collect())
}

fn probe_detail(program: &str, stdout: &[u8], stderr: &[u8]) -> Option<String> {
    if program == "glxinfo" {
        let details = String::from_utf8_lossy(stdout)
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with("OpenGL vendor string:")
                    || line.starts_with("OpenGL renderer string:")
                    || line.starts_with("OpenGL core profile version string:")
            })
            .take(3)
            .collect::<Vec<_>>()
            .join("; ");
        if !details.is_empty() {
            return Some(details);
        }
    }
    first_nonempty_line(stdout).or_else(|| first_nonempty_line(stderr))
}

fn software_renderer_detected(probe: &Probe) -> bool {
    probe.detail.as_ref().is_some_and(|detail| {
        let lower = detail.to_ascii_lowercase();
        lower.contains("llvmpipe") || lower.contains("softpipe")
    })
}

fn hardware_renderer_detected(probe: &Probe) -> bool {
    probe.detail.as_ref().is_some_and(|detail| {
        let lower = detail.to_ascii_lowercase();
        !lower.contains("llvmpipe")
            && !lower.contains("softpipe")
            && (lower.contains("d3d12")
                || lower.contains("nvidia")
                || lower.contains("radeon")
                || lower.contains("intel"))
    })
}

fn gazebo_renderer_log_probe(skipped: bool) -> Probe {
    let command = "read ~/.gz/rendering/ogre2.log".to_string();
    if skipped {
        return Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Skipped,
            command,
            detail: None,
        };
    }

    let Some(home) = env::var_os("HOME") else {
        return Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Unavailable,
            command,
            detail: Some("HOME is unavailable".into()),
        };
    };
    let path = Path::new(&home).join(".gz/rendering/ogre2.log");
    match fs::read_to_string(&path) {
        Ok(log) => Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Available,
            command,
            detail: gazebo_renderer_detail(&log)
                .or_else(|| Some("log exists but has no renderer identity".into())),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Unavailable,
            command,
            detail: Some("run a Gazebo rendering sensor once to create this log".into()),
        },
        Err(error) => Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Failed,
            command,
            detail: Some(error.to_string()),
        },
    }
}

/// Return the renderer identity written by Gazebo during the current run.
///
/// The modification-time guard prevents a previous Gazebo process from being
/// mistaken for the renderer used by this benchmark.
pub fn renderer_identity_since(not_before: SystemTime) -> Result<Option<RendererIdentity>, String> {
    let Some(home) = env::var_os("HOME") else {
        return Ok(None);
    };
    let path = Path::new(&home).join(".gz/rendering/ogre2.log");
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not inspect Gazebo renderer log {}: {error}",
                path.display()
            ));
        }
    };
    let modified = metadata.modified().map_err(|error| {
        format!(
            "could not read modification time for Gazebo renderer log {}: {error}",
            path.display()
        )
    })?;
    if modified < not_before {
        return Ok(None);
    }
    let log = fs::read_to_string(&path).map_err(|error| {
        format!(
            "could not read Gazebo renderer log {}: {error}",
            path.display()
        )
    })?;
    let modified_unix_ms = modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    Ok(parse_renderer_identity(
        &log,
        path.display().to_string(),
        modified_unix_ms,
    ))
}

fn parse_renderer_identity(
    log: &str,
    source: String,
    log_modified_unix_ms: Option<u64>,
) -> Option<RendererIdentity> {
    let egl_device = renderer_value(log, "EGL Device:");
    let gl_renderer = renderer_value(log, "GL_RENDERER =");
    let device_name = renderer_value(log, "Device Name:");
    if egl_device.is_none() && gl_renderer.is_none() && device_name.is_none() {
        return None;
    }
    let software = [&egl_device, &gl_renderer, &device_name]
        .into_iter()
        .flatten()
        .any(|value| {
            let lower = value.to_ascii_lowercase();
            lower.contains("llvmpipe") || lower.contains("softpipe")
        });
    Some(RendererIdentity {
        source,
        egl_device,
        gl_renderer,
        device_name,
        software,
        log_modified_unix_ms,
    })
}

fn renderer_value(log: &str, marker: &str) -> Option<String> {
    log.lines().rev().find_map(|line| {
        let (_, value) = line.split_once(marker)?;
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn gazebo_renderer_detail(log: &str) -> Option<String> {
    let details = ["EGL Device:", "GL_RENDERER =", "Device Name:"]
        .into_iter()
        .filter_map(|marker| {
            log.lines()
                .rev()
                .find(|line| line.contains(marker))
                .map(str::trim)
        })
        .collect::<Vec<_>>();
    (!details.is_empty()).then(|| details.join("; "))
}

fn cpu_model() -> Option<String> {
    if let Some(cpuinfo) = read_optional("/proc/cpuinfo") {
        if let Some(model) = cpuinfo.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            (key == "model name" || key == "Hardware").then(|| value.trim().to_string())
        }) {
            return Some(model);
        }
    }
    env::var("PROCESSOR_IDENTIFIER").ok()
}

fn linux_memory_total_bytes() -> Option<u64> {
    let meminfo = read_optional("/proc/meminfo")?;
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    kib.checked_mul(1024)
}

fn read_optional(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn cpu_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse4.2") {
            features.push("sse4.2");
        }
        if std::is_x86_feature_detected!("avx") {
            features.push("avx");
        }
        if std::is_x86_feature_detected!("avx2") {
            features.push("avx2");
        }
        if std::is_x86_feature_detected!("fma") {
            features.push("fma");
        }
        if std::is_x86_feature_detected!("avx512f") {
            features.push("avx512f");
        }
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_wsl_before_container() {
        assert_eq!(
            classify_platform("linux", Some("5.15.0-microsoft-standard-WSL2"), true),
            PlatformKind::Wsl
        );
    }

    #[test]
    fn classifies_native_linux() {
        assert_eq!(
            classify_platform("linux", Some("6.8.0-generic"), false),
            PlatformKind::LinuxNative
        );
    }

    #[test]
    fn skipped_probe_never_executes() {
        let result = probe("command-that-must-not-exist", &["--version"], true);
        assert!(matches!(result.state, ProbeState::Skipped));
    }

    #[test]
    fn extracts_renderer_from_glxinfo() {
        let output = b"name of display: :0\nOpenGL vendor string: Mesa\nOpenGL renderer string: D3D12 (GPU)\n";
        assert_eq!(
            probe_detail("glxinfo", output, b""),
            Some("OpenGL vendor string: Mesa; OpenGL renderer string: D3D12 (GPU)".into())
        );
    }

    #[test]
    fn recognizes_software_renderer() {
        let probe = Probe {
            name: "glxinfo",
            state: ProbeState::Available,
            command: "glxinfo -B".into(),
            detail: Some("OpenGL renderer string: llvmpipe (LLVM 20.1.2)".into()),
        };
        assert!(software_renderer_detected(&probe));
    }

    #[test]
    fn recognizes_gazebo_d3d12_renderer_as_hardware() {
        let probe = Probe {
            name: "gazebo-renderer-log",
            state: ProbeState::Available,
            command: "read ~/.gz/rendering/ogre2.log".into(),
            detail: Some("GL_RENDERER = D3D12 (NVIDIA GeForce RTX 3050 Ti Laptop GPU)".into()),
        };
        assert!(hardware_renderer_detected(&probe));
        assert!(!software_renderer_detected(&probe));
    }

    #[test]
    fn extracts_gazebo_egl_renderer_from_log() {
        let log = "12:00:00: EGL Device: EGL_MESA_device_software\n\
                   12:00:00: GL_RENDERER = llvmpipe (LLVM 20.1.2)\n\
                   12:00:01: Device Name: llvmpipe (LLVM 20.1.2)\n";
        assert_eq!(
            gazebo_renderer_detail(log),
            Some(
                "12:00:00: EGL Device: EGL_MESA_device_software; 12:00:00: GL_RENDERER = llvmpipe (LLVM 20.1.2); 12:00:01: Device Name: llvmpipe (LLVM 20.1.2)".into()
            )
        );
    }

    #[test]
    fn parses_structured_gazebo_renderer_identity() {
        let log = "12:00:00: EGL Device: EGL_MESA_device_software\n\
                   12:00:00: GL_RENDERER = llvmpipe (LLVM 20.1.2)\n\
                   12:00:01: Device Name: llvmpipe (LLVM 20.1.2)\n";
        let identity = parse_renderer_identity(log, "ogre2.log".into(), Some(123)).unwrap();
        assert_eq!(
            identity.egl_device.as_deref(),
            Some("EGL_MESA_device_software")
        );
        assert_eq!(
            identity.gl_renderer.as_deref(),
            Some("llvmpipe (LLVM 20.1.2)")
        );
        assert_eq!(
            identity.device_name.as_deref(),
            Some("llvmpipe (LLVM 20.1.2)")
        );
        assert!(identity.software);
        assert_eq!(identity.log_modified_unix_ms, Some(123));
    }

    #[test]
    fn parses_hardware_renderer_without_false_software_flag() {
        let log = "12:00:00: EGL Device: D3D12 (NVIDIA)\n\
                   12:00:00: GL_RENDERER = D3D12 (NVIDIA GeForce RTX 3050 Ti)\n";
        let identity = parse_renderer_identity(log, "ogre2.log".into(), None).unwrap();
        assert!(!identity.software);
    }
}
