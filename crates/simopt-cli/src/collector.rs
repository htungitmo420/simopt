use serde_json::Value;
use simopt_core::TimeSample;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct WorldStats {
    sim_time_s: f64,
    paused: bool,
}

enum ReaderEvent {
    Stats(WorldStats),
    InvalidJson(String),
    ReadError(String),
    EndOfStream,
}

pub fn collect(
    gz_command: &Path,
    topic: &str,
    duration_seconds: f64,
    output: &Path,
) -> Result<usize, String> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err("duration must be finite and greater than zero".into());
    }
    if topic.trim().is_empty() {
        return Err("stats topic cannot be empty".into());
    }

    let mut child = Command::new(gz_command)
        .args(["topic", "-e", "-t", topic, "--json-output"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start {}: {error}; install Gazebo or pass --gz-command",
                gz_command.display()
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture gz stdout".to_string())?;

    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) if line.trim().is_empty() => continue,
                Ok(line) => match parse_world_stats_json(&line) {
                    Ok(stats) => {
                        if sender.send(ReaderEvent::Stats(stats)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        if sender
                            .send(ReaderEvent::InvalidJson(format!("{error}; input={line}")))
                            .is_err()
                        {
                            return;
                        }
                    }
                },
                Err(error) => {
                    let _ = sender.send(ReaderEvent::ReadError(error.to_string()));
                    return;
                }
            }
        }
        let _ = sender.send(ReaderEvent::EndOfStream);
    });

    let duration = Duration::from_secs_f64(duration_seconds);
    let started = Instant::now();
    let mut samples = Vec::new();
    let mut invalid_messages = 0usize;
    let mut first_parse_error = None;
    let mut stream_error = None;

    while started.elapsed() < duration {
        let remaining = duration.saturating_sub(started.elapsed());
        let wait = remaining.min(Duration::from_millis(250));
        match receiver.recv_timeout(wait) {
            Ok(ReaderEvent::Stats(stats)) => samples.push(TimeSample {
                wall_time_s: started.elapsed().as_secs_f64(),
                sim_time_s: stats.sim_time_s,
                paused: stats.paused,
            }),
            Ok(ReaderEvent::InvalidJson(error)) => {
                invalid_messages += 1;
                first_parse_error.get_or_insert(error);
            }
            Ok(ReaderEvent::ReadError(error)) => {
                stream_error = Some(error);
                break;
            }
            Ok(ReaderEvent::EndOfStream) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if child
                    .try_wait()
                    .map_err(|error| format!("could not inspect gz process: {error}"))?
                    .is_some()
                {
                    break;
                }
            }
        }
    }

    if child
        .try_wait()
        .map_err(|error| format!("could not inspect gz process: {error}"))?
        .is_none()
    {
        child
            .kill()
            .map_err(|error| format!("could not stop gz collector: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("could not wait for gz collector: {error}"))?;
    reader
        .join()
        .map_err(|_| "gz output reader panicked".to_string())?;

    if let Some(error) = stream_error {
        return Err(format!("could not read Gazebo stats: {error}"));
    }
    if samples.len() < 2 {
        let parse_detail = first_parse_error
            .map(|error| format!(" First parse error: {error}"))
            .unwrap_or_default();
        return Err(format!(
            "captured only {} valid sample(s) and {invalid_messages} invalid message(s) from {topic}; \
             ensure Gazebo is running, the world is unpaused, and the topic carries gz.msgs.WorldStatistics.\
             {parse_detail} Collector exit status: {status}",
            samples.len()
        ));
    }

    write_samples(output, &samples)?;
    if invalid_messages > 0 {
        eprintln!(
            "warning: ignored {invalid_messages} invalid JSON message(s); first error: {}",
            first_parse_error.unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(samples.len())
}

fn write_samples(output: &Path, samples: &[TimeSample]) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
    }
    let file = File::create(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    let mut writer = csv::Writer::from_writer(file);
    for sample in samples {
        writer
            .serialize(sample)
            .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    }
    writer
        .flush()
        .map_err(|error| format!("could not flush {}: {error}", output.display()))
}

fn parse_world_stats_json(input: &str) -> Result<WorldStats, String> {
    let value: Value = serde_json::from_str(input).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_string())?;
    let sim_time = object
        .get("simTime")
        .or_else(|| object.get("sim_time"))
        .ok_or_else(|| "missing simTime".to_string())?;
    let sim_time = sim_time
        .as_object()
        .ok_or_else(|| "simTime must be an object".to_string())?;
    // Protobuf JSON omits scalar fields that contain their default value. In
    // practice Gazebo therefore emits only `nsec` during the first simulated
    // second instead of serializing `sec: 0`.
    let seconds = sim_time
        .get("sec")
        .map(json_number)
        .transpose()
        .map_err(|error| format!("invalid simTime.sec: {error}"))?
        .unwrap_or(0.0);
    let nanoseconds = sim_time
        .get("nsec")
        .or_else(|| sim_time.get("nanos"))
        .map(json_number)
        .transpose()?
        .unwrap_or(0.0);
    if !(0.0..1_000_000_000.0).contains(&nanoseconds) {
        return Err("simTime nanoseconds must be in [0, 1e9)".into());
    }
    let paused = object
        .get("paused")
        .map(json_bool)
        .transpose()?
        .unwrap_or(false);
    Ok(WorldStats {
        sim_time_s: seconds + nanoseconds / 1_000_000_000.0,
        paused,
    })
}

fn json_number(value: &Value) -> Result<f64, String> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(number) => number.parse::<f64>().ok(),
        _ => None,
    }
    .ok_or_else(|| "expected a finite number or numeric string".to_string())?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err("number must be finite".into())
    }
}

fn json_bool(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err("expected a boolean or boolean string".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protobuf_json_camel_case_and_string_integers() {
        let stats = parse_world_stats_json(
            r#"{"simTime":{"sec":"12","nsec":250000000},"paused":false,"iterations":"10"}"#,
        )
        .unwrap();
        assert_eq!(
            stats,
            WorldStats {
                sim_time_s: 12.25,
                paused: false
            }
        );
    }

    #[test]
    fn parses_snake_case_and_missing_nanos() {
        let stats = parse_world_stats_json(r#"{"sim_time":{"sec":3},"paused":"true"}"#).unwrap();
        assert_eq!(
            stats,
            WorldStats {
                sim_time_s: 3.0,
                paused: true
            }
        );
    }

    #[test]
    fn treats_omitted_protobuf_seconds_as_zero() {
        let stats = parse_world_stats_json(
            r#"{"simTime":{"nsec":99000000},"realTimeFactor":0.999,"iterations":"99"}"#,
        )
        .unwrap();
        assert_eq!(
            stats,
            WorldStats {
                sim_time_s: 0.099,
                paused: false
            }
        );
    }

    #[test]
    fn rejects_out_of_range_nanoseconds() {
        let error =
            parse_world_stats_json(r#"{"simTime":{"sec":3,"nsec":1000000000},"paused":false}"#)
                .unwrap_err();
        assert!(error.contains("nanoseconds"));
    }
}
