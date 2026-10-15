use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simopt_core::{
    aggregate_reports, analyze_samples, ArtifactFingerprint, BenchmarkFingerprint,
    BenchmarkSuiteReport, SourceRevision,
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const SCENARIO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioManifest {
    pub schema_version: u32,
    pub label: String,
    pub launch: LaunchCommand,
    pub stats_topic: String,
    #[serde(default)]
    pub required_topics: Vec<String>,
    #[serde(default)]
    pub message_check_topics: Vec<String>,
    #[serde(default)]
    pub load_topics: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    #[serde(default = "default_gz_command")]
    pub gz_command: PathBuf,
    #[serde(default = "default_warmup_seconds")]
    pub warmup_seconds: f64,
    #[serde(default = "default_duration_seconds")]
    pub duration_seconds: f64,
    #[serde(default = "default_repeats")]
    pub repeats: usize,
    #[serde(default = "default_startup_timeout_seconds")]
    pub startup_timeout_seconds: f64,
    #[serde(default = "default_message_timeout_seconds")]
    pub message_timeout_seconds: f64,
    #[serde(default = "default_shutdown_grace_seconds")]
    pub shutdown_grace_seconds: f64,
    #[serde(default)]
    pub inter_run_delay_seconds: f64,
    #[serde(default = "default_window_seconds")]
    pub window_seconds: f64,
    #[serde(default = "default_collect_process_metrics")]
    pub collect_process_metrics: bool,
    #[serde(default = "default_resource_sample_interval_ms")]
    pub resource_sample_interval_ms: u64,
    #[serde(default = "default_collect_capabilities")]
    pub collect_capabilities: bool,
    #[serde(default)]
    pub fingerprint_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchCommand {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

pub fn run(scenario_path: &Path, output_override: Option<&Path>) -> Result<PathBuf, String> {
    let manifest_text = fs::read_to_string(scenario_path)
        .map_err(|error| format!("could not read {}: {error}", scenario_path.display()))?;
    let manifest: ScenarioManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("invalid scenario {}: {error}", scenario_path.display()))?;
    validate_manifest(&manifest)?;

    let scenario_directory = scenario_path.parent().unwrap_or_else(|| Path::new("."));
    let working_directory = resolve_working_directory(scenario_directory, &manifest);
    let output_directory = output_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| Path::new("reports").join(&manifest.label));
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "could not create benchmark output {}: {error}",
            output_directory.display()
        )
    })?;
    fs::write(output_directory.join("scenario.json"), manifest_text).map_err(|error| {
        format!(
            "could not save scenario snapshot in {}: {error}",
            output_directory.display()
        )
    })?;

    let capability_report = if manifest.collect_capabilities {
        let path = output_directory.join("capabilities.json");
        crate::write_json(Some(&path), &crate::doctor::collect(false))?;
        Some(fingerprint_file(&path, Path::new("capabilities.json"))?)
    } else {
        None
    };
    let workload_files = manifest
        .fingerprint_files
        .iter()
        .map(|path| {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                working_directory.join(path)
            };
            fingerprint_file(&resolved, path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fingerprint = BenchmarkFingerprint {
        algorithm: "sha256".into(),
        scenario: fingerprint_file(scenario_path, scenario_path)?,
        workload_files,
        capability_report,
        tool_version: Some(env!("CARGO_PKG_VERSION").into()),
        source_revision: source_revision(scenario_path),
    };

    let mut reports = Vec::with_capacity(manifest.repeats);
    for run_index in 1..=manifest.repeats {
        let run_label = format!("{}-run-{run_index:03}", manifest.label);
        let run_prefix = output_directory.join(format!("run-{run_index:03}"));
        let log_path = run_prefix.with_extension("log");
        let csv_path = run_prefix.with_extension("csv");
        let report_path = run_prefix.with_extension("json");

        eprintln!(
            "starting {run_label} ({run_index}/{}) with command: {} {}",
            manifest.repeats,
            manifest.launch.program.display(),
            manifest.launch.args.join(" ")
        );
        let mut process = ManagedProcess::spawn(&manifest, &working_directory, &log_path)?;
        let ready_topics = std::iter::once(manifest.stats_topic.as_str())
            .chain(manifest.required_topics.iter().map(String::as_str))
            .chain(manifest.message_check_topics.iter().map(String::as_str))
            .chain(manifest.load_topics.iter().map(String::as_str))
            .collect::<Vec<_>>();
        wait_for_topics(
            &manifest.gz_command,
            &ready_topics,
            Duration::from_secs_f64(manifest.startup_timeout_seconds),
            &mut process,
        )?;
        let mut load_subscribers =
            TopicSubscribers::start(&manifest.gz_command, &manifest.load_topics)?;
        verify_topic_messages(
            &manifest.gz_command,
            &manifest.message_check_topics,
            Duration::from_secs_f64(manifest.message_timeout_seconds),
            &mut process,
        )?;
        wait_while_running(
            &mut process,
            Duration::from_secs_f64(manifest.warmup_seconds),
            "warm-up",
        )?;
        load_subscribers.ensure_running()?;

        let resource_monitor = manifest.collect_process_metrics.then(|| {
            crate::resources::ResourceMonitor::start(
                process.id(),
                Duration::from_millis(manifest.resource_sample_interval_ms),
            )
        });
        let collection_result = crate::collector::collect(
            &manifest.gz_command,
            &manifest.stats_topic,
            manifest.duration_seconds,
            &csv_path,
        );
        let process_metrics = resource_monitor
            .map(crate::resources::ResourceMonitor::stop)
            .transpose()?;
        let renderer_result = crate::doctor::renderer_identity_since(process.started_at());
        let sample_count = collection_result?;
        load_subscribers.ensure_running()?;
        load_subscribers.stop();
        process.terminate(Duration::from_secs_f64(manifest.shutdown_grace_seconds))?;

        let samples = crate::read_samples(&csv_path)?;
        let mut report = analyze_samples(
            &run_label,
            csv_path.display().to_string(),
            &samples,
            manifest.window_seconds,
        )
        .map_err(|error| error.to_string())?;
        report.process_metrics = process_metrics;
        match renderer_result {
            Ok(Some(renderer)) => {
                if renderer.software {
                    report.warnings.push(
                        "Gazebo used a software renderer for this run; camera and GPU LiDAR results are not representative of GPU-accelerated execution"
                            .into(),
                    );
                }
                report.renderer = Some(renderer);
            }
            Ok(None) => report
                .warnings
                .push("no fresh Gazebo Ogre2 renderer identity was observed for this run".into()),
            Err(error) => report.warnings.push(format!(
                "could not capture Gazebo renderer identity: {error}"
            )),
        }
        crate::write_json(Some(&report_path), &report)?;
        eprintln!(
            "completed {run_label}: {sample_count} samples, RTF {:.4}",
            report.total_rtf
        );
        reports.push(report);

        if run_index < manifest.repeats && manifest.inter_run_delay_seconds > 0.0 {
            thread::sleep(Duration::from_secs_f64(manifest.inter_run_delay_seconds));
        }
    }

    let mut suite = aggregate_reports(
        &manifest.label,
        scenario_path.display().to_string(),
        &reports,
    )
    .map_err(|error| error.to_string())?;
    suite.fingerprint = Some(fingerprint);
    let suite_path = output_directory.join("suite.json");
    crate::write_json(Some(&suite_path), &suite)?;
    print_summary(&suite);
    Ok(suite_path)
}

fn validate_manifest(manifest: &ScenarioManifest) -> Result<(), String> {
    if manifest.schema_version != SCENARIO_SCHEMA_VERSION {
        return Err(format!(
            "unsupported scenario schema version {}; expected {SCENARIO_SCHEMA_VERSION}",
            manifest.schema_version
        ));
    }
    if manifest.label.is_empty()
        || !manifest
            .label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(
            "scenario label must contain only ASCII letters, numbers, dot, underscore, or dash"
                .into(),
        );
    }
    if manifest.launch.program.as_os_str().is_empty() {
        return Err("launch program cannot be empty".into());
    }
    if manifest.stats_topic.trim().is_empty() {
        return Err("stats_topic cannot be empty".into());
    }
    if manifest
        .required_topics
        .iter()
        .any(|topic| topic.trim().is_empty())
    {
        return Err("required_topics cannot contain an empty topic".into());
    }
    if manifest
        .message_check_topics
        .iter()
        .any(|topic| topic.trim().is_empty())
    {
        return Err("message_check_topics cannot contain an empty topic".into());
    }
    if manifest
        .load_topics
        .iter()
        .any(|topic| topic.trim().is_empty())
    {
        return Err("load_topics cannot contain an empty topic".into());
    }
    if manifest.repeats == 0 {
        return Err("repeats must be greater than zero".into());
    }
    validate_nonnegative("warmup_seconds", manifest.warmup_seconds)?;
    validate_positive("duration_seconds", manifest.duration_seconds)?;
    validate_positive("startup_timeout_seconds", manifest.startup_timeout_seconds)?;
    validate_positive("message_timeout_seconds", manifest.message_timeout_seconds)?;
    validate_positive("shutdown_grace_seconds", manifest.shutdown_grace_seconds)?;
    validate_nonnegative("inter_run_delay_seconds", manifest.inter_run_delay_seconds)?;
    validate_positive("window_seconds", manifest.window_seconds)?;
    if manifest.resource_sample_interval_ms == 0 {
        return Err("resource_sample_interval_ms must be greater than zero".into());
    }
    if manifest
        .fingerprint_files
        .iter()
        .any(|path| path.as_os_str().is_empty())
    {
        return Err("fingerprint_files cannot contain an empty path".into());
    }
    Ok(())
}

fn fingerprint_file(path: &Path, recorded_path: &Path) -> Result<ArtifactFingerprint, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        format!(
            "could not inspect fingerprint file {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "fingerprint input is not a regular file: {}",
            path.display()
        ));
    }
    let mut file = File::open(path).map_err(|error| {
        format!(
            "could not open fingerprint file {}: {error}",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(ArtifactFingerprint {
        path: recorded_path.display().to_string(),
        size_bytes: metadata.len(),
        digest: format!("{:x}", hasher.finalize()),
    })
}

fn source_revision(path: &Path) -> Option<SourceRevision> {
    let canonical = fs::canonicalize(path).ok()?;
    let repository = canonical
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())?;
    let commit_output = Command::new("git")
        .args(["-C", repository.to_str()?, "rev-parse", "HEAD"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !commit_output.status.success() {
        return None;
    }
    let commit = String::from_utf8(commit_output.stdout)
        .ok()?
        .trim()
        .to_string();
    if commit.is_empty() {
        return None;
    }
    let status_output = Command::new("git")
        .args([
            "-C",
            repository.to_str()?,
            "status",
            "--porcelain",
            "--untracked-files=no",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some(SourceRevision {
        commit,
        dirty: !status_output.stdout.is_empty() || !status_output.status.success(),
    })
}

fn validate_positive(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{name} must be finite and greater than zero"))
    }
}

fn validate_nonnegative(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(format!("{name} must be finite and non-negative"))
    }
}

fn resolve_working_directory(base: &Path, manifest: &ScenarioManifest) -> PathBuf {
    match &manifest.working_directory {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => base.join(path),
        None => base.to_path_buf(),
    }
}

fn wait_for_topics(
    gz_command: &Path,
    topics: &[&str],
    timeout: Duration,
    process: &mut ManagedProcess,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        process.ensure_running("topic discovery")?;
        let output = Command::new(gz_command)
            .args(["topic", "-l"])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("could not run {} topic -l: {error}", gz_command.display()))?;
        if output.status.success() {
            let topic_output = String::from_utf8_lossy(&output.stdout);
            let listed = topic_output.lines().map(str::trim).collect::<Vec<_>>();
            if topics.iter().all(|topic| listed.contains(topic)) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            let missing = topics.join(", ");
            return Err(format!(
                "timed out waiting for Gazebo topics: {missing}; inspect {}",
                process.log_path.display()
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn wait_while_running(
    process: &mut ManagedProcess,
    duration: Duration,
    phase: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        process.ensure_running(phase)?;
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
    Ok(())
}

fn verify_topic_messages(
    gz_command: &Path,
    topics: &[String],
    timeout: Duration,
    process: &mut ManagedProcess,
) -> Result<(), String> {
    for topic in topics {
        process.ensure_running("sensor message readiness")?;
        let mut command = Command::new(gz_command);
        command
            .args(["topic", "-e", "-t", topic, "-n", "1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        let mut subscriber = command.spawn().map_err(|error| {
            format!("could not subscribe to required Gazebo topic {topic}: {error}")
        })?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Err(error) = process.ensure_running("sensor message readiness") {
                let _ = terminate_child(&mut subscriber, true);
                let _ = subscriber.wait();
                return Err(error);
            }
            match subscriber.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(format!(
                        "Gazebo topic check for {topic} exited with {status}"
                    ));
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(50));
                }
                Ok(None) => {
                    let _ = terminate_child(&mut subscriber, true);
                    let _ = subscriber.wait();
                    return Err(format!(
                        "timed out after {:.1}s waiting for a message on Gazebo topic {topic}",
                        timeout.as_secs_f64()
                    ));
                }
                Err(error) => {
                    let _ = terminate_child(&mut subscriber, true);
                    let _ = subscriber.wait();
                    return Err(format!(
                        "could not inspect Gazebo topic check for {topic}: {error}"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn print_summary(suite: &BenchmarkSuiteReport) {
    eprintln!(
        "benchmark {} complete: {} run(s), median total RTF {:.4}, p05 across runs {:.4}",
        suite.label, suite.run_count, suite.total_rtf.median, suite.total_rtf.p05
    );
}

struct TopicSubscribers {
    children: Vec<(String, Child)>,
}

impl TopicSubscribers {
    fn start(gz_command: &Path, topics: &[String]) -> Result<Self, String> {
        let mut subscribers = Self {
            children: Vec::with_capacity(topics.len()),
        };
        for topic in topics {
            let mut command = Command::new(gz_command);
            command
                .args(["topic", "-e", "-t", topic])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            command.process_group(0);
            let child = command.spawn().map_err(|error| {
                format!("could not start persistent subscriber for {topic}: {error}")
            })?;
            subscribers.children.push((topic.clone(), child));
        }
        Ok(subscribers)
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        for (topic, child) in &mut self.children {
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    return Err(format!(
                        "persistent subscriber for {topic} exited early with {status}"
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "could not inspect persistent subscriber for {topic}: {error}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        for (_, child) in &mut self.children {
            let _ = terminate_child(child, true);
            let _ = child.wait();
        }
        self.children.clear();
    }
}

impl Drop for TopicSubscribers {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ManagedProcess {
    child: Child,
    log_path: PathBuf,
    started_at: SystemTime,
    terminated: bool,
}

impl ManagedProcess {
    fn spawn(
        manifest: &ScenarioManifest,
        working_directory: &Path,
        log_path: &Path,
    ) -> Result<Self, String> {
        let log = File::create(log_path)
            .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
        let stderr = log
            .try_clone()
            .map_err(|error| format!("could not clone {}: {error}", log_path.display()))?;
        let mut command = Command::new(&manifest.launch.program);
        command
            .args(&manifest.launch.args)
            .envs(&manifest.launch.environment)
            .current_dir(working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        command.process_group(0);

        let started_at = SystemTime::now();
        let child = command.spawn().map_err(|error| {
            format!(
                "could not start {} in {}: {error}",
                manifest.launch.program.display(),
                working_directory.display()
            )
        })?;
        Ok(Self {
            child,
            log_path: log_path.to_path_buf(),
            started_at,
            terminated: false,
        })
    }

    fn ensure_running(&mut self, phase: &str) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => Err(format!(
                "launch process exited during {phase} with {status}; inspect {}",
                self.log_path.display()
            )),
            Err(error) => Err(format!("could not inspect launch process: {error}")),
        }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn started_at(&self) -> SystemTime {
        self.started_at
    }

    fn terminate(&mut self, grace: Duration) -> Result<(), String> {
        if self.terminated {
            return Ok(());
        }
        if self
            .child
            .try_wait()
            .map_err(|error| format!("could not inspect launch process: {error}"))?
            .is_some()
        {
            self.terminated = true;
            return Ok(());
        }

        terminate_child(&mut self.child, false)?;
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self
                .child
                .try_wait()
                .map_err(|error| format!("could not inspect launch process: {error}"))?
                .is_some()
            {
                self.terminated = true;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        terminate_child(&mut self.child, true)?;
        self.child
            .wait()
            .map_err(|error| format!("could not wait for launch process: {error}"))?;
        self.terminated = true;
        Ok(())
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let _ = self.terminate(Duration::from_secs(1));
    }
}

#[cfg(unix)]
fn terminate_child(child: &mut Child, force: bool) -> Result<(), String> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let process_group = format!("-{}", child.id());
    let status = Command::new("kill")
        .args([signal, "--", &process_group])
        .status()
        .map_err(|error| format!("could not signal launch process group: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not send {signal} to launch process group {}: {status}",
            child.id()
        ))
    }
}

#[cfg(not(unix))]
fn terminate_child(child: &mut Child, _force: bool) -> Result<(), String> {
    child
        .kill()
        .map_err(|error| format!("could not stop launch process: {error}"))
}

fn default_gz_command() -> PathBuf {
    PathBuf::from("gz")
}

fn default_warmup_seconds() -> f64 {
    30.0
}

fn default_duration_seconds() -> f64 {
    180.0
}

fn default_repeats() -> usize {
    3
}

fn default_startup_timeout_seconds() -> f64 {
    30.0
}

fn default_message_timeout_seconds() -> f64 {
    10.0
}

fn default_shutdown_grace_seconds() -> f64 {
    5.0
}

fn default_window_seconds() -> f64 {
    1.0
}

fn default_collect_process_metrics() -> bool {
    true
}

fn default_resource_sample_interval_ms() -> u64 {
    250
}

fn default_collect_capabilities() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ScenarioManifest {
        ScenarioManifest {
            schema_version: 1,
            label: "minimal-smoke".into(),
            launch: LaunchCommand {
                program: "gz".into(),
                args: vec!["sim".into()],
                environment: BTreeMap::new(),
            },
            stats_topic: "/world/default/stats".into(),
            required_topics: Vec::new(),
            message_check_topics: Vec::new(),
            load_topics: Vec::new(),
            working_directory: Some("../..".into()),
            gz_command: "gz".into(),
            warmup_seconds: 1.0,
            duration_seconds: 2.0,
            repeats: 2,
            startup_timeout_seconds: 5.0,
            message_timeout_seconds: 2.0,
            shutdown_grace_seconds: 1.0,
            inter_run_delay_seconds: 0.0,
            window_seconds: 1.0,
            collect_process_metrics: true,
            resource_sample_interval_ms: 250,
            collect_capabilities: true,
            fingerprint_files: vec!["examples/minimal-world.sdf".into()],
        }
    }

    #[test]
    fn validates_a_minimal_scenario() {
        assert_eq!(validate_manifest(&manifest()), Ok(()));
    }

    #[test]
    fn rejects_unsafe_label_and_zero_repeats() {
        let mut invalid = manifest();
        invalid.label = "../../report".into();
        invalid.repeats = 0;
        assert!(validate_manifest(&invalid)
            .unwrap_err()
            .contains("scenario label"));
    }

    #[test]
    fn resolves_working_directory_from_manifest_location() {
        assert_eq!(
            resolve_working_directory(Path::new("configs/scenarios"), &manifest()),
            PathBuf::from("configs/scenarios/../..")
        );
    }

    #[test]
    fn serde_defaults_are_stable() {
        let parsed: ScenarioManifest = serde_json::from_str(
            r#"{
              "schema_version": 1,
              "label": "defaulted",
              "launch": { "program": "gz" },
              "stats_topic": "/world/default/stats"
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.repeats, 3);
        assert_eq!(parsed.warmup_seconds, 30.0);
        assert_eq!(parsed.duration_seconds, 180.0);
        assert_eq!(parsed.message_timeout_seconds, 10.0);
        assert!(parsed.collect_process_metrics);
        assert_eq!(parsed.resource_sample_interval_ms, 250);
        assert!(parsed.collect_capabilities);
        assert!(parsed.fingerprint_files.is_empty());
        assert!(parsed.message_check_topics.is_empty());
    }

    #[test]
    fn computes_sha256_fingerprint() {
        let path = std::env::temp_dir().join(format!(
            "simopt-fingerprint-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"abc").unwrap();
        let fingerprint = fingerprint_file(&path, Path::new("input.txt")).unwrap();
        let _ = fs::remove_file(path);

        assert_eq!(fingerprint.path, "input.txt");
        assert_eq!(fingerprint.size_bytes, 3);
        assert_eq!(
            fingerprint.digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
