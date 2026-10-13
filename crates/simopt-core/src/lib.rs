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
}
