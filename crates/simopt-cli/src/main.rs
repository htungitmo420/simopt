mod bench;
mod collector;
mod doctor;
mod resources;

use clap::{Parser, Subcommand};
use serde::Deserialize;
use simopt_core::{analyze_samples, compare_reports, BenchmarkReport, TimeSample};
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Parser)]
#[command(
    name = "simopt",
    version,
    about = "Measure before optimizing Gazebo simulations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect platform and Gazebo / ROS capabilities without changing the system.
    Doctor {
        /// Save JSON to this file instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Skip external commands; useful for offline CI and restricted shells.
        #[arg(long)]
        skip_probes: bool,
    },
    /// Capture monotonic wall time and simulation time from a Gazebo stats topic.
    Collect {
        /// Gazebo world name; ignored when --topic is provided.
        #[arg(long, default_value = "default")]
        world: String,
        /// Explicit Gazebo WorldStatistics topic.
        #[arg(long)]
        topic: Option<String>,
        /// Capture duration measured by this process's monotonic clock.
        #[arg(long, default_value_t = 180.0)]
        duration_seconds: f64,
        /// CSV output consumed by `simopt analyze`.
        #[arg(short, long)]
        output: PathBuf,
        /// Path or command name for the Gazebo CLI.
        #[arg(long, default_value = "gz")]
        gz_command: PathBuf,
    },
    /// Convert timestamp samples from CSV into an RTF report.
    Analyze {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "run")]
        label: String,
        #[arg(long, default_value_t = 1.0)]
        window_seconds: f64,
    },
    /// Compare two reports produced by `simopt analyze`.
    Compare {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 10.0)]
        minimum_gain_percent: f64,
    },
    /// Launch and repeat a complete Gazebo benchmark from a JSON scenario.
    Bench {
        /// Scenario manifest describing the launch command and measurement policy.
        #[arg(short, long)]
        scenario: PathBuf,
        /// Directory for the scenario snapshot, logs, samples, and reports.
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Deserialize)]
struct CsvSample {
    wall_time_s: f64,
    sim_time_s: f64,
    #[serde(default)]
    paused: bool,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Doctor {
            output,
            skip_probes,
        } => write_json(output.as_deref(), &doctor::collect(skip_probes)),
        Command::Collect {
            world,
            topic,
            duration_seconds,
            output,
            gz_command,
        } => {
            let topic = topic.unwrap_or_else(|| format!("/world/{world}/stats"));
            let sample_count = collector::collect(&gz_command, &topic, duration_seconds, &output)?;
            eprintln!(
                "captured {sample_count} sample(s) from {topic} into {}",
                output.display()
            );
            Ok(())
        }
        Command::Analyze {
            input,
            output,
            label,
            window_seconds,
        } => {
            let samples = read_samples(&input)?;
            let report =
                analyze_samples(label, input.display().to_string(), &samples, window_seconds)
                    .map_err(|error| error.to_string())?;
            write_json(output.as_deref(), &report)
        }
        Command::Compare {
            baseline,
            candidate,
            output,
            minimum_gain_percent,
        } => {
            let baseline_report: BenchmarkReport = read_json(&baseline)?;
            let candidate_report: BenchmarkReport = read_json(&candidate)?;
            let comparison =
                compare_reports(&baseline_report, &candidate_report, minimum_gain_percent)
                    .map_err(|error| error.to_string())?;
            write_json(output.as_deref(), &comparison)
        }
        Command::Bench {
            scenario,
            output_dir,
        } => {
            let suite_path = bench::run(&scenario, output_dir.as_deref())?;
            eprintln!("wrote benchmark suite to {}", suite_path.display());
            Ok(())
        }
    }
}

pub(crate) fn read_samples(path: &Path) -> Result<Vec<TimeSample>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut samples = Vec::new();
    for (row, result) in reader.deserialize::<CsvSample>().enumerate() {
        let value = result.map_err(|error| {
            format!("invalid CSV row {} in {}: {error}", row + 2, path.display())
        })?;
        samples.push(TimeSample {
            wall_time_s: value.wall_time_s,
            sim_time_s: value.sim_time_s,
            paused: value.paused,
        });
    }
    Ok(samples)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))
}

pub(crate) fn write_json<T: serde::Serialize>(
    path: Option<&Path>,
    value: &T,
) -> Result<(), String> {
    match path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("could not create {}: {error}", parent.display())
                    })?;
                }
            }
            let file = File::create(path)
                .map_err(|error| format!("could not create {}: {error}", path.display()))?;
            serde_json::to_writer_pretty(BufWriter::new(file), value)
                .map_err(|error| format!("could not write {}: {error}", path.display()))
        }
        None => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            serde_json::to_writer_pretty(&mut handle, value)
                .map_err(|error| format!("could not write JSON: {error}"))?;
            writeln!(handle).map_err(|error| format!("could not write JSON: {error}"))
        }
    }
}
