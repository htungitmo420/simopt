//! Portable measurement and comparison logic for Gazebo simulation benchmarks.
//!
//! This crate deliberately has no Gazebo, ROS, GPU, or operating-system APIs.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeSample {
    pub wall_time_s: f64,
    pub sim_time_s: f64,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub count: usize,
    pub min: f64,
    pub p05: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub label: String,
    pub source: String,
    pub sample_count: usize,
    pub valid_interval_count: usize,
    pub excluded_interval_count: usize,
    pub clock_reset_count: usize,
    pub total_wall_time_s: f64,
    pub total_sim_time_s: f64,
    pub total_rtf: f64,
    pub window_seconds: f64,
    pub window_rtf: Distribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_metrics: Option<ProcessMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<RendererIdentity>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessMetrics {
    pub source: String,
    pub sample_count: usize,
    pub wall_time_s: f64,
    pub cpu_time_s: Option<f64>,
    pub average_cpu_cores: Option<f64>,
    pub peak_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_process_count: Option<usize>,
    pub warnings: Vec<String>,
}

/// Renderer identity observed from the Gazebo Ogre2 log for this run.
///
/// This is stored per run because WSL graphics selection can differ between
/// otherwise identical launches. A pre-run capability probe can describe the
/// environment, but it cannot prove which renderer the benchmark used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RendererIdentity {
    pub source: String,
    pub egl_device: Option<String>,
    pub gl_renderer: Option<String>,
    pub device_name: Option<String>,
    pub software: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_modified_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub schema_version: u32,
    pub baseline_label: String,
    pub candidate_label: String,
    pub baseline_rtf: f64,
    pub candidate_rtf: f64,
    pub absolute_rtf_change: f64,
    pub relative_rtf_change_percent: Option<f64>,
    pub minimum_gain_percent: f64,
    pub performance_gate_passed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadState {
    RealtimeCapped,
    BelowRealtime,
    FasterThanRealtime,
    Unstable,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteDiagnosticReport {
    pub schema_version: u32,
    pub label: String,
    pub run_count: usize,
    pub workload_state: WorkloadState,
    pub median_total_rtf: f64,
    pub p05_total_rtf: f64,
    pub relative_run_spread_percent: Option<f64>,
    pub median_average_cpu_cores: Option<f64>,
    pub maximum_peak_rss_bytes: Option<u64>,
    pub renderers: Vec<String>,
    pub software_renderer_runs: usize,
    pub findings: Vec<String>,
    pub recommended_next_steps: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteComparisonReport {
    pub schema_version: u32,
    pub baseline_label: String,
    pub candidate_label: String,
    pub baseline_run_count: usize,
    pub candidate_run_count: usize,
    pub baseline_median_rtf: f64,
    pub candidate_median_rtf: f64,
    pub absolute_median_rtf_change: f64,
    pub relative_median_rtf_change_percent: Option<f64>,
    pub minimum_gain_percent: f64,
    pub workload_fingerprint_match: Option<bool>,
    pub renderer_match: Option<bool>,
    pub measurement_policy_match: bool,
    pub comparable: bool,
    pub performance_gate_passed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkSuiteReport {
    pub schema_version: u32,
    pub label: String,
    pub scenario: String,
    pub run_count: usize,
    pub total_rtf: Distribution,
    pub window_median_rtf: Distribution,
    pub window_p05_rtf: Distribution,
    pub runs: Vec<BenchmarkReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<BenchmarkFingerprint>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkFingerprint {
    pub algorithm: String,
    pub scenario: ArtifactFingerprint,
    pub workload_files: Vec<ArtifactFingerprint>,
    pub capability_report: Option<ArtifactFingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRevision {
    pub commit: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactFingerprint {
    pub path: String,
    pub size_bytes: u64,
    pub digest: String,
}

#[derive(Debug, Error, PartialEq)]
pub enum AnalysisError {
    #[error("at least two samples are required")]
    NotEnoughSamples,
    #[error("window duration must be finite and greater than zero")]
    InvalidWindow,
    #[error("sample {index} contains a non-finite timestamp")]
    NonFiniteSample { index: usize },
    #[error("no advancing simulation intervals were found")]
    NoAdvancingIntervals,
    #[error("reports use unsupported schema versions: baseline={baseline}, candidate={candidate}")]
    UnsupportedSchema { baseline: u32, candidate: u32 },
    #[error("minimum gain must be finite and non-negative")]
    InvalidMinimumGain,
    #[error("at least one benchmark report is required")]
    NoBenchmarkReports,
    #[error("report {index} uses unsupported schema version {version}")]
    UnsupportedReportSchema { index: usize, version: u32 },
    #[error("suite uses unsupported schema version {version}")]
    UnsupportedSuiteSchema { version: u32 },
    #[error("suite declares {declared} run(s) but contains {actual}")]
    InvalidSuiteRunCount { declared: usize, actual: usize },
}

/// Analyze monotonic wall/simulation timestamps.
///
/// Paused intervals, non-advancing wall time, and simulation clock resets are
/// excluded. A reset starts a new segment and is never bridged by an RTF window.
pub fn analyze_samples(
    label: impl Into<String>,
    source: impl Into<String>,
    samples: &[TimeSample],
    window_seconds: f64,
) -> Result<BenchmarkReport, AnalysisError> {
    if samples.len() < 2 {
        return Err(AnalysisError::NotEnoughSamples);
    }
    if !window_seconds.is_finite() || window_seconds <= 0.0 {
        return Err(AnalysisError::InvalidWindow);
    }
    for (index, sample) in samples.iter().enumerate() {
        if !sample.wall_time_s.is_finite() || !sample.sim_time_s.is_finite() {
            return Err(AnalysisError::NonFiniteSample { index });
        }
    }

    let mut valid_interval_count = 0usize;
    let mut excluded_interval_count = 0usize;
    let mut clock_reset_count = 0usize;
    let mut total_wall_time_s = 0.0;
    let mut total_sim_time_s = 0.0;
    let mut windows = Vec::new();
    let mut window_wall = 0.0;
    let mut window_sim = 0.0;

    for pair in samples.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        let wall_delta = current.wall_time_s - previous.wall_time_s;
        let sim_delta = current.sim_time_s - previous.sim_time_s;

        if sim_delta < 0.0 {
            clock_reset_count += 1;
            excluded_interval_count += 1;
            discard_partial_window(&mut window_wall, &mut window_sim);
            continue;
        }
        if previous.paused || current.paused || wall_delta <= 0.0 || sim_delta <= 0.0 {
            excluded_interval_count += 1;
            discard_partial_window(&mut window_wall, &mut window_sim);
            continue;
        }

        valid_interval_count += 1;
        total_wall_time_s += wall_delta;
        total_sim_time_s += sim_delta;
        window_wall += wall_delta;
        window_sim += sim_delta;

        if window_wall >= window_seconds {
            windows.push(window_sim / window_wall);
            window_wall = 0.0;
            window_sim = 0.0;
        }
    }

    discard_partial_window(&mut window_wall, &mut window_sim);

    if valid_interval_count == 0 || total_wall_time_s <= 0.0 {
        return Err(AnalysisError::NoAdvancingIntervals);
    }

    let mut warnings = Vec::new();
    if clock_reset_count > 0 {
        warnings.push(format!(
            "simulation clock reset {clock_reset_count} time(s); reset intervals were excluded"
        ));
    }
    if excluded_interval_count > 0 {
        warnings.push(format!(
            "excluded {excluded_interval_count} paused, invalid, or non-advancing interval(s)"
        ));
    }
    if windows.is_empty() {
        windows.push(total_sim_time_s / total_wall_time_s);
        warnings.push(
            "capture contains no complete RTF window; distribution uses the total interval".into(),
        );
    }
    if windows.len() < 3 {
        warnings
            .push("fewer than three RTF windows; repeat the benchmark for a stable result".into());
    }

    Ok(BenchmarkReport {
        schema_version: REPORT_SCHEMA_VERSION,
        label: label.into(),
        source: source.into(),
        sample_count: samples.len(),
        valid_interval_count,
        excluded_interval_count,
        clock_reset_count,
        total_wall_time_s,
        total_sim_time_s,
        total_rtf: total_sim_time_s / total_wall_time_s,
        window_seconds,
        window_rtf: distribution(&mut windows),
        process_metrics: None,
        renderer: None,
        warnings,
    })
}

pub fn compare_reports(
    baseline: &BenchmarkReport,
    candidate: &BenchmarkReport,
    minimum_gain_percent: f64,
) -> Result<ComparisonReport, AnalysisError> {
    if baseline.schema_version != REPORT_SCHEMA_VERSION
        || candidate.schema_version != REPORT_SCHEMA_VERSION
    {
        return Err(AnalysisError::UnsupportedSchema {
            baseline: baseline.schema_version,
            candidate: candidate.schema_version,
        });
    }
    if !minimum_gain_percent.is_finite() || minimum_gain_percent < 0.0 {
        return Err(AnalysisError::InvalidMinimumGain);
    }

    let absolute = candidate.total_rtf - baseline.total_rtf;
    let relative = if baseline.total_rtf > 0.0 {
        Some(absolute / baseline.total_rtf * 100.0)
    } else {
        None
    };
    let mut warnings = Vec::new();
    if baseline.window_seconds != candidate.window_seconds {
        warnings.push("reports use different RTF window durations".into());
    }
    if baseline.total_rtf <= 0.0 {
        warnings.push("baseline RTF is not positive; relative gain is undefined".into());
    }
    warnings.extend(
        baseline
            .warnings
            .iter()
            .map(|warning| format!("baseline: {warning}")),
    );
    warnings.extend(
        candidate
            .warnings
            .iter()
            .map(|warning| format!("candidate: {warning}")),
    );

    Ok(ComparisonReport {
        schema_version: REPORT_SCHEMA_VERSION,
        baseline_label: baseline.label.clone(),
        candidate_label: candidate.label.clone(),
        baseline_rtf: baseline.total_rtf,
        candidate_rtf: candidate.total_rtf,
        absolute_rtf_change: absolute,
        relative_rtf_change_percent: relative,
        minimum_gain_percent,
        performance_gate_passed: relative.is_some_and(|value| value >= minimum_gain_percent),
        warnings,
    })
}

/// Summarize what a repeated benchmark can and cannot establish.
pub fn diagnose_suite(
    suite: &BenchmarkSuiteReport,
) -> Result<SuiteDiagnosticReport, AnalysisError> {
    validate_suite(suite)?;

    let median_rtf = suite.total_rtf.median;
    let spread = (median_rtf > 0.0)
        .then(|| (suite.total_rtf.max - suite.total_rtf.min).abs() / median_rtf * 100.0);
    let unstable =
        spread.is_some_and(|value| value > 5.0) || suite.window_p05_rtf.median < median_rtf * 0.90;
    let workload_state = if unstable {
        WorkloadState::Unstable
    } else if (0.97..=1.03).contains(&median_rtf) && suite.total_rtf.p05 >= 0.95 {
        WorkloadState::RealtimeCapped
    } else if median_rtf < 0.95 {
        WorkloadState::BelowRealtime
    } else if median_rtf > 1.03 {
        WorkloadState::FasterThanRealtime
    } else {
        WorkloadState::Inconclusive
    };

    let mut cpu_cores = suite
        .runs
        .iter()
        .filter_map(|run| run.process_metrics.as_ref()?.average_cpu_cores)
        .collect::<Vec<_>>();
    let median_average_cpu_cores = median_value(&mut cpu_cores);
    let maximum_peak_rss_bytes = suite
        .runs
        .iter()
        .filter_map(|run| run.process_metrics.as_ref()?.peak_rss_bytes)
        .max();
    let renderers = suite_renderer_names(suite);
    let software_renderer_runs = suite
        .runs
        .iter()
        .filter_map(|run| run.renderer.as_ref())
        .filter(|renderer| renderer.software)
        .count();

    let mut findings = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut warnings = suite.warnings.clone();
    match workload_state {
        WorkloadState::RealtimeCapped => {
            findings.push(
                "RTF is tightly clustered around 1.0; this workload reaches the configured real-time target and does not expose available performance headroom"
                    .into(),
            );
            recommended_next_steps.push(
                "run the real drone workload or a heavier reference profile before selecting an optimization"
                    .into(),
            );
            recommended_next_steps.push(
                "use an uncapped profile only as a separate throughput experiment; keep the capped profile for real-time behavior validation"
                    .into(),
            );
        }
        WorkloadState::BelowRealtime => {
            findings
                .push("median RTF is below 0.95, so the workload cannot sustain real time".into());
            recommended_next_steps.push(
                "run the B2-B7 isolation matrix to separate physics, sensing, bridge, perception, controller, and SITL cost"
                    .into(),
            );
        }
        WorkloadState::FasterThanRealtime => {
            findings.push(
                "median RTF is above 1.03; the scenario appears to run without a real-time cap"
                    .into(),
            );
            recommended_next_steps.push(
                "treat this as throughput evidence and use a real-time-capped run for latency and behavior validation"
                    .into(),
            );
        }
        WorkloadState::Unstable => {
            findings.push(
                "run-to-run or lower-tail RTF variation exceeds the stability threshold".into(),
            );
            recommended_next_steps.push(
                "inspect per-run logs and background load, lengthen warm-up, and repeat before comparing candidates"
                    .into(),
            );
        }
        WorkloadState::Inconclusive => {
            findings.push(
                "RTF is near the real-time boundary but does not meet a stable classification"
                    .into(),
            );
            recommended_next_steps
                .push("repeat with longer runs and inspect the one-second RTF tail".into());
        }
    }
    if let Some(value) = median_average_cpu_cores {
        findings.push(format!(
            "the measured process tree used a median average of {value:.3} CPU cores"
        ));
        if value <= f64::EPSILON {
            warnings.push(
                "process metrics report zero CPU use; the launcher may have detached Gazebo workers outside the observed process tree"
                    .into(),
            );
            recommended_next_steps.push(
                "repeat with the process-tree collector before using CPU or RSS metrics for a GUI/headless comparison"
                    .into(),
            );
        }
    }
    if software_renderer_runs > 0 {
        findings.push(format!(
            "{software_renderer_runs} run(s) used a software renderer"
        ));
        recommended_next_steps.push(
            "establish hardware rendering before attributing camera or GPU LiDAR cost".into(),
        );
    } else if !renderers.is_empty() {
        findings.push(format!("Gazebo renderer: {}", renderers.join(", ")));
    }
    if suite.run_count < 3 {
        recommended_next_steps
            .push("collect at least three repetitions before making a performance decision".into());
    }

    Ok(SuiteDiagnosticReport {
        schema_version: REPORT_SCHEMA_VERSION,
        label: suite.label.clone(),
        run_count: suite.run_count,
        workload_state,
        median_total_rtf: median_rtf,
        p05_total_rtf: suite.total_rtf.p05,
        relative_run_spread_percent: spread,
        median_average_cpu_cores,
        maximum_peak_rss_bytes,
        renderers,
        software_renderer_runs,
        findings,
        recommended_next_steps,
        warnings,
    })
}

/// Compare the median of repeated benchmark suites while checking provenance.
pub fn compare_suites(
    baseline: &BenchmarkSuiteReport,
    candidate: &BenchmarkSuiteReport,
    minimum_gain_percent: f64,
) -> Result<SuiteComparisonReport, AnalysisError> {
    validate_suite(baseline)?;
    validate_suite(candidate)?;
    if !minimum_gain_percent.is_finite() || minimum_gain_percent < 0.0 {
        return Err(AnalysisError::InvalidMinimumGain);
    }

    let baseline_rtf = baseline.total_rtf.median;
    let candidate_rtf = candidate.total_rtf.median;
    let absolute = candidate_rtf - baseline_rtf;
    let relative = (baseline_rtf > 0.0).then(|| absolute / baseline_rtf * 100.0);
    let workload_fingerprint_match = match (&baseline.fingerprint, &candidate.fingerprint) {
        (Some(left), Some(right)) => Some(workload_digests(left) == workload_digests(right)),
        _ => None,
    };
    let baseline_renderers = suite_renderer_names(baseline);
    let candidate_renderers = suite_renderer_names(candidate);
    let renderer_match = if baseline_renderers.is_empty() || candidate_renderers.is_empty() {
        None
    } else {
        Some(baseline_renderers == candidate_renderers)
    };
    let baseline_window = baseline.runs.first().map(|run| run.window_seconds);
    let candidate_window = candidate.runs.first().map(|run| run.window_seconds);
    let mut baseline_durations = baseline
        .runs
        .iter()
        .map(|run| run.total_wall_time_s)
        .collect::<Vec<_>>();
    let mut candidate_durations = candidate
        .runs
        .iter()
        .map(|run| run.total_wall_time_s)
        .collect::<Vec<_>>();
    let baseline_duration = median_value(&mut baseline_durations).unwrap_or_default();
    let candidate_duration = median_value(&mut candidate_durations).unwrap_or_default();
    let duration_match = baseline_duration > 0.0
        && ((candidate_duration - baseline_duration).abs() / baseline_duration) <= 0.05;
    let measurement_policy_match = baseline_window == candidate_window && duration_match;

    let mut comparable = true;
    let mut warnings = Vec::new();
    match workload_fingerprint_match {
        Some(true) => {}
        Some(false) => {
            comparable = false;
            warnings.push("workload file fingerprints differ; performance gate is invalid".into());
        }
        None => {
            comparable = false;
            warnings
                .push("workload fingerprints are unavailable; performance gate is invalid".into());
        }
    }
    match renderer_match {
        Some(true) => {}
        Some(false) => {
            comparable = false;
            warnings.push("Gazebo renderer identities differ; performance gate is invalid".into());
        }
        None => {
            comparable = false;
            warnings.push("renderer identity is unavailable; performance gate is invalid".into());
        }
    }
    if !measurement_policy_match {
        comparable = false;
        warnings.push(
            "RTF window duration or measured wall duration differs by more than 5%; performance gate is invalid"
                .into(),
        );
    }
    if baseline.run_count < 3 || candidate.run_count < 3 {
        warnings.push("both suites should contain at least three repetitions".into());
    }
    if baseline.run_count != candidate.run_count {
        warnings.push("suites contain different repetition counts".into());
    }
    if (0.97..=1.03).contains(&baseline_rtf) && (0.97..=1.03).contains(&candidate_rtf) {
        warnings.push(
            "both suites are near the real-time cap; this comparison may not expose performance headroom"
                .into(),
        );
    }
    warnings.extend(
        baseline
            .warnings
            .iter()
            .map(|warning| format!("baseline: {warning}")),
    );
    warnings.extend(
        candidate
            .warnings
            .iter()
            .map(|warning| format!("candidate: {warning}")),
    );

    Ok(SuiteComparisonReport {
        schema_version: REPORT_SCHEMA_VERSION,
        baseline_label: baseline.label.clone(),
        candidate_label: candidate.label.clone(),
        baseline_run_count: baseline.run_count,
        candidate_run_count: candidate.run_count,
        baseline_median_rtf: baseline_rtf,
        candidate_median_rtf: candidate_rtf,
        absolute_median_rtf_change: absolute,
        relative_median_rtf_change_percent: relative,
        minimum_gain_percent,
        workload_fingerprint_match,
        renderer_match,
        measurement_policy_match,
        comparable,
        performance_gate_passed: comparable
            && relative.is_some_and(|value| value >= minimum_gain_percent),
        warnings,
    })
}

fn validate_suite(suite: &BenchmarkSuiteReport) -> Result<(), AnalysisError> {
    if suite.schema_version != REPORT_SCHEMA_VERSION {
        return Err(AnalysisError::UnsupportedSuiteSchema {
            version: suite.schema_version,
        });
    }
    if suite.run_count != suite.runs.len() {
        return Err(AnalysisError::InvalidSuiteRunCount {
            declared: suite.run_count,
            actual: suite.runs.len(),
        });
    }
    Ok(())
}

fn suite_renderer_names(suite: &BenchmarkSuiteReport) -> Vec<String> {
    let mut names = suite
        .runs
        .iter()
        .filter_map(|run| run.renderer.as_ref())
        .filter_map(|renderer| {
            renderer
                .gl_renderer
                .as_ref()
                .or(renderer.device_name.as_ref())
        })
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn workload_digests(fingerprint: &BenchmarkFingerprint) -> Vec<String> {
    let mut digests = fingerprint
        .workload_files
        .iter()
        .map(|artifact| artifact.digest.clone())
        .collect::<Vec<_>>();
    digests.sort();
    digests
}

fn median_value(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(percentile(values, 0.5))
}

/// Aggregate repeated runs without hiding the individual reports.
pub fn aggregate_reports(
    label: impl Into<String>,
    scenario: impl Into<String>,
    reports: &[BenchmarkReport],
) -> Result<BenchmarkSuiteReport, AnalysisError> {
    if reports.is_empty() {
        return Err(AnalysisError::NoBenchmarkReports);
    }
    for (index, report) in reports.iter().enumerate() {
        if report.schema_version != REPORT_SCHEMA_VERSION {
            return Err(AnalysisError::UnsupportedReportSchema {
                index,
                version: report.schema_version,
            });
        }
    }

    let mut total_rtf = reports
        .iter()
        .map(|report| report.total_rtf)
        .collect::<Vec<_>>();
    let mut window_median_rtf = reports
        .iter()
        .map(|report| report.window_rtf.median)
        .collect::<Vec<_>>();
    let mut window_p05_rtf = reports
        .iter()
        .map(|report| report.window_rtf.p05)
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    if reports.len() < 3 {
        warnings.push(
            "fewer than three repetitions; use at least three for performance decisions".into(),
        );
    }
    let first_window = reports[0].window_seconds;
    if reports
        .iter()
        .any(|report| report.window_seconds != first_window)
    {
        warnings.push("runs use different RTF window durations".into());
    }
    let missing_renderer_count = reports
        .iter()
        .filter(|report| report.renderer.is_none())
        .count();
    if missing_renderer_count > 0 {
        warnings.push(format!(
            "{missing_renderer_count} run(s) have no fresh Gazebo renderer identity"
        ));
    }
    if reports
        .iter()
        .filter_map(|report| report.renderer.as_ref())
        .any(|renderer| renderer.software)
    {
        warnings.push("at least one run used a software renderer".into());
    }
    let mut renderer_names = reports
        .iter()
        .filter_map(|report| report.renderer.as_ref())
        .filter_map(|renderer| {
            renderer
                .gl_renderer
                .as_ref()
                .or(renderer.device_name.as_ref())
        })
        .cloned()
        .collect::<Vec<_>>();
    renderer_names.sort();
    renderer_names.dedup();
    if renderer_names.len() > 1 {
        warnings.push(format!(
            "runs used different Gazebo renderers: {}",
            renderer_names.join(", ")
        ));
    }

    Ok(BenchmarkSuiteReport {
        schema_version: REPORT_SCHEMA_VERSION,
        label: label.into(),
        scenario: scenario.into(),
        run_count: reports.len(),
        total_rtf: distribution(&mut total_rtf),
        window_median_rtf: distribution(&mut window_median_rtf),
        window_p05_rtf: distribution(&mut window_p05_rtf),
        runs: reports.to_vec(),
        fingerprint: None,
        warnings,
    })
}

fn discard_partial_window(wall: &mut f64, sim: &mut f64) {
    *wall = 0.0;
    *sim = 0.0;
}

fn distribution(values: &mut [f64]) -> Distribution {
    values.sort_by(f64::total_cmp);
    Distribution {
        count: values.len(),
        min: values[0],
        p05: percentile(values, 0.05),
        median: percentile(values, 0.50),
        p95: percentile(values, 0.95),
        max: values[values.len() - 1],
    }
}

fn percentile(sorted: &[f64], probability: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let index = probability * (sorted.len() - 1) as f64;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let fraction = index - lower as f64;
    sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(wall: f64, sim: f64) -> TimeSample {
        TimeSample {
            wall_time_s: wall,
            sim_time_s: sim,
            paused: false,
        }
    }

    #[test]
    fn computes_total_and_window_rtf() {
        let samples = [
            sample(0.0, 0.0),
            sample(1.0, 0.3),
            sample(2.0, 0.7),
            sample(3.0, 0.9),
        ];
        let report = analyze_samples("baseline", "test.csv", &samples, 1.0).unwrap();

        assert!((report.total_rtf - 0.3).abs() < 1e-12);
        assert_eq!(report.window_rtf.count, 3);
        assert!((report.window_rtf.median - 0.3).abs() < 1e-12);
        assert!((report.window_rtf.p05 - 0.21).abs() < 1e-12);
    }

    #[test]
    fn excludes_pause_and_clock_reset_without_bridging_segments() {
        let samples = [
            sample(0.0, 0.0),
            sample(1.0, 0.5),
            TimeSample {
                wall_time_s: 2.0,
                sim_time_s: 0.5,
                paused: true,
            },
            sample(3.0, 0.5),
            sample(4.0, 1.0),
            sample(5.0, 0.0),
            sample(6.0, 0.25),
        ];
        let report = analyze_samples("run", "test.csv", &samples, 1.0).unwrap();

        assert_eq!(report.clock_reset_count, 1);
        assert_eq!(report.valid_interval_count, 3);
        assert_eq!(report.excluded_interval_count, 3);
        assert!((report.total_rtf - (1.25 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn comparison_uses_relative_gain() {
        let baseline = analyze_samples(
            "baseline",
            "base.csv",
            &[sample(0.0, 0.0), sample(1.0, 0.3)],
            1.0,
        )
        .unwrap();
        let candidate = analyze_samples(
            "candidate",
            "candidate.csv",
            &[sample(0.0, 0.0), sample(1.0, 0.36)],
            1.0,
        )
        .unwrap();

        let comparison = compare_reports(&baseline, &candidate, 10.0).unwrap();
        assert!((comparison.relative_rtf_change_percent.unwrap() - 20.0).abs() < 1e-12);
        assert!(comparison.performance_gate_passed);
    }

    #[test]
    fn rejects_non_finite_input() {
        let error = analyze_samples(
            "bad",
            "bad.csv",
            &[sample(0.0, 0.0), sample(f64::NAN, 1.0)],
            1.0,
        )
        .unwrap_err();
        assert_eq!(error, AnalysisError::NonFiniteSample { index: 1 });
    }

    #[test]
    fn aggregates_repeated_runs_and_preserves_them() {
        let run_one = analyze_samples(
            "run-1",
            "one.csv",
            &[sample(0.0, 0.0), sample(1.0, 0.5)],
            1.0,
        )
        .unwrap();
        let run_two = analyze_samples(
            "run-2",
            "two.csv",
            &[sample(0.0, 0.0), sample(1.0, 0.7)],
            1.0,
        )
        .unwrap();

        let suite = aggregate_reports("suite", "scenario.json", &[run_one, run_two]).unwrap();
        assert_eq!(suite.run_count, 2);
        assert_eq!(suite.runs.len(), 2);
        assert!((suite.total_rtf.median - 0.6).abs() < 1e-12);
        assert!(suite
            .warnings
            .iter()
            .any(|warning| warning.contains("three")));
    }

    #[test]
    fn warns_when_repeated_runs_change_renderer() {
        let mut run_one = analyze_samples(
            "run-1",
            "one.csv",
            &[sample(0.0, 0.0), sample(1.0, 0.5)],
            1.0,
        )
        .unwrap();
        let mut run_two = run_one.clone();
        run_two.label = "run-2".into();
        run_one.renderer = Some(RendererIdentity {
            source: "ogre2.log".into(),
            egl_device: None,
            gl_renderer: Some("D3D12 (NVIDIA)".into()),
            device_name: None,
            software: false,
            log_modified_unix_ms: None,
        });
        run_two.renderer = Some(RendererIdentity {
            source: "ogre2.log".into(),
            egl_device: Some("EGL_MESA_device_software".into()),
            gl_renderer: Some("llvmpipe".into()),
            device_name: None,
            software: true,
            log_modified_unix_ms: None,
        });

        let suite = aggregate_reports("suite", "scenario.json", &[run_one, run_two]).unwrap();
        assert!(suite
            .warnings
            .iter()
            .any(|warning| warning.contains("software renderer")));
        assert!(suite
            .warnings
            .iter()
            .any(|warning| warning.contains("different Gazebo renderers")));
    }

    #[test]
    fn diagnoses_a_light_realtime_capped_suite() {
        let mut runs = Vec::new();
        for index in 0..3 {
            let mut run = analyze_samples(
                format!("run-{index}"),
                "run.csv",
                &[sample(0.0, 0.0), sample(10.0, 10.0)],
                1.0,
            )
            .unwrap();
            run.process_metrics = Some(ProcessMetrics {
                source: "procfs".into(),
                sample_count: 10,
                wall_time_s: 10.0,
                cpu_time_s: Some(4.0),
                average_cpu_cores: Some(0.4),
                peak_rss_bytes: Some(300_000_000),
                peak_process_count: Some(4),
                warnings: Vec::new(),
            });
            run.renderer = Some(RendererIdentity {
                source: "ogre2.log".into(),
                egl_device: None,
                gl_renderer: Some("D3D12 (NVIDIA)".into()),
                device_name: None,
                software: false,
                log_modified_unix_ms: None,
            });
            runs.push(run);
        }
        let suite = aggregate_reports("light", "scenario.json", &runs).unwrap();
        let diagnostic = diagnose_suite(&suite).unwrap();

        assert_eq!(diagnostic.workload_state, WorkloadState::RealtimeCapped);
        assert_eq!(diagnostic.median_average_cpu_cores, Some(0.4));
        assert_eq!(diagnostic.maximum_peak_rss_bytes, Some(300_000_000));
        assert!(diagnostic
            .recommended_next_steps
            .iter()
            .any(|step| step.contains("heavier")));
    }

    #[test]
    fn warns_when_suite_reports_zero_process_cpu() {
        let mut run = analyze_samples(
            "gui-run",
            "run.csv",
            &[sample(0.0, 0.0), sample(10.0, 10.0)],
            1.0,
        )
        .unwrap();
        run.process_metrics = Some(ProcessMetrics {
            source: "linux-procfs-process-group".into(),
            sample_count: 40,
            wall_time_s: 10.0,
            cpu_time_s: Some(0.0),
            average_cpu_cores: Some(0.0),
            peak_rss_bytes: Some(64_000_000),
            peak_process_count: None,
            warnings: Vec::new(),
        });
        let suite = aggregate_reports("gui", "scenario.json", &[run]).unwrap();
        let diagnostic = diagnose_suite(&suite).unwrap();

        assert!(diagnostic
            .warnings
            .iter()
            .any(|warning| warning.contains("zero CPU")));
        assert!(diagnostic
            .recommended_next_steps
            .iter()
            .any(|step| step.contains("process-tree collector")));
    }

    #[test]
    fn suite_comparison_requires_matching_workload_and_renderer() {
        let report =
            analyze_samples("run", "run.csv", &[sample(0.0, 0.0), sample(1.0, 0.5)], 1.0).unwrap();
        let mut baseline =
            aggregate_reports("baseline", "baseline.json", &[report.clone()]).unwrap();
        let mut candidate = aggregate_reports("candidate", "candidate.json", &[report]).unwrap();
        let fingerprint = |digest: &str| BenchmarkFingerprint {
            algorithm: "sha256".into(),
            scenario: ArtifactFingerprint {
                path: "scenario.json".into(),
                size_bytes: 1,
                digest: "scenario".into(),
            },
            workload_files: vec![ArtifactFingerprint {
                path: "world.sdf".into(),
                size_bytes: 1,
                digest: digest.into(),
            }],
            capability_report: None,
            tool_version: Some("0.1.0".into()),
            source_revision: None,
        };
        baseline.fingerprint = Some(fingerprint("world-a"));
        candidate.fingerprint = Some(fingerprint("world-b"));

        let comparison = compare_suites(&baseline, &candidate, 5.0).unwrap();
        assert_eq!(comparison.workload_fingerprint_match, Some(false));
        assert!(comparison.measurement_policy_match);
        assert!(!comparison.comparable);
        assert!(!comparison.performance_gate_passed);
    }
}
