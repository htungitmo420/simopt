use simopt_core::ProcessMetrics;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::{
    collections::{HashMap, HashSet},
    fs,
    process::{Command, Stdio},
};

pub struct ResourceMonitor {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<ProcessMetrics>>,
}

impl ResourceMonitor {
    pub fn start(process_group: u32, interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker =
            thread::spawn(move || monitor_process_group(process_group, interval, worker_stop));
        Self {
            stop,
            worker: Some(worker),
        }
    }

    pub fn stop(mut self) -> Result<ProcessMetrics, String> {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .expect("resource monitor worker must exist")
            .join()
            .map_err(|_| "resource monitor panicked".to_string())
    }
}

impl Drop for ResourceMonitor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.stop.store(true, Ordering::Relaxed);
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn monitor_process_group(
    process_group: u32,
    interval: Duration,
    stop: Arc<AtomicBool>,
) -> ProcessMetrics {
    let started = Instant::now();
    let clock_ticks = clock_ticks_per_second();
    let mut previous_ticks = HashMap::new();
    let mut accumulated_ticks = 0u64;
    let mut peak_rss_bytes = 0u64;
    let mut sample_count = 0usize;
    let mut warnings = Vec::new();

    loop {
        match sample_linux_process_group(process_group) {
            Ok(processes) => {
                let mut current_pids = HashSet::new();
                let mut rss_bytes = 0u64;
                for process in processes {
                    current_pids.insert(process.pid);
                    rss_bytes = rss_bytes.saturating_add(process.rss_bytes.unwrap_or(0));
                    if let Some(previous) = previous_ticks.insert(process.pid, process.cpu_ticks) {
                        accumulated_ticks = accumulated_ticks
                            .saturating_add(process.cpu_ticks.saturating_sub(previous));
                    }
                }
                previous_ticks.retain(|pid, _| current_pids.contains(pid));
                peak_rss_bytes = peak_rss_bytes.max(rss_bytes);
                sample_count += 1;
            }
            Err(error) if warnings.is_empty() => warnings.push(error),
            Err(_) => {}
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }
        thread::sleep(interval);
    }

    let wall_time_s = started.elapsed().as_secs_f64();
    let cpu_time_s =
        clock_ticks.map(|ticks_per_second| accumulated_ticks as f64 / ticks_per_second);
    let average_cpu_cores =
        cpu_time_s.and_then(|value| (wall_time_s > 0.0).then_some(value / wall_time_s));
    if clock_ticks.is_none() {
        warnings.push("getconf CLK_TCK was unavailable; CPU time could not be calculated".into());
    }
    if sample_count == 0 {
        warnings.push("no Linux process-group resource samples were collected".into());
    }

    ProcessMetrics {
        source: "linux-procfs-process-group".into(),
        sample_count,
        wall_time_s,
        cpu_time_s,
        average_cpu_cores,
        peak_rss_bytes: (sample_count > 0).then_some(peak_rss_bytes),
        warnings,
    }
}

#[cfg(not(target_os = "linux"))]
fn monitor_process_group(
    _process_group: u32,
    interval: Duration,
    stop: Arc<AtomicBool>,
) -> ProcessMetrics {
    let started = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(interval);
    }
    ProcessMetrics {
        source: "unavailable".into(),
        sample_count: 0,
        wall_time_s: started.elapsed().as_secs_f64(),
        cpu_time_s: None,
        average_cpu_cores: None,
        peak_rss_bytes: None,
        warnings: vec!["process-group metrics are currently implemented only on Linux".into()],
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq)]
struct LinuxProcessSample {
    pid: u32,
    process_group: u32,
    cpu_ticks: u64,
    rss_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
fn sample_linux_process_group(process_group: u32) -> Result<Vec<LinuxProcessSample>, String> {
    let entries =
        fs::read_dir("/proc").map_err(|error| format!("could not read /proc: {error}"))?;
    let mut samples = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = match fs::read_to_string(entry.path().join("stat")) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some((sample_group, cpu_ticks)) = parse_proc_stat(&stat) else {
            continue;
        };
        if sample_group != process_group {
            continue;
        }
        let rss_bytes = fs::read_to_string(entry.path().join("status"))
            .ok()
            .and_then(|status| parse_vm_rss_bytes(&status));
        samples.push(LinuxProcessSample {
            pid,
            process_group: sample_group,
            cpu_ticks,
            rss_bytes,
        });
    }
    Ok(samples)
}

#[cfg(target_os = "linux")]
fn parse_proc_stat(stat: &str) -> Option<(u32, u64)> {
    let command_end = stat.rfind(')')?;
    let fields = stat
        .get(command_end + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let process_group = fields.get(2)?.parse().ok()?;
    let user_ticks = fields.get(11)?.parse::<u64>().ok()?;
    let system_ticks = fields.get(12)?.parse::<u64>().ok()?;
    Some((process_group, user_ticks.saturating_add(system_ticks)))
}

#[cfg(target_os = "linux")]
fn parse_vm_rss_bytes(status: &str) -> Option<u64> {
    let kib = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?;
        value.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    kib.checked_mul(1024)
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second() -> Option<f64> {
    let output = Command::new("getconf")
        .arg("CLK_TCK")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_with_spaces_in_command() {
        let stat = "123 (gz sim server) S 1 456 456 0 -1 0 0 0 0 0 120 30 0 0 20 0 4 0 1 1000 20";
        assert_eq!(parse_proc_stat(stat), Some((456, 150)));
    }

    #[test]
    fn parses_rss_from_proc_status() {
        let status = "Name:\tgz\nVmSize:\t1000 kB\nVmRSS:\t2048 kB\n";
        assert_eq!(parse_vm_rss_bytes(status), Some(2_097_152));
    }
}
