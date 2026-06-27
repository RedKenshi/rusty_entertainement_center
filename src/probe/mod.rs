//! Video metadata probing via ffprobe (from ffmpeg).
//!
//! If ffprobe is not installed, all probe fields stay `None`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

static FFPROBE_AVAILABLE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VideoProbe {
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub codec: Option<String>,
}

fn ffprobe_available() -> bool {
    *FFPROBE_AVAILABLE.get_or_init(|| {
        Command::new("ffprobe")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

/// Probes duration, container bitrate, and primary video codec in a single ffprobe call.
pub fn probe_video(path: &Path) -> VideoProbe {
    if !ffprobe_available() {
        return VideoProbe::default();
    }

    let Some(path_str) = path.to_str() else {
        return VideoProbe::default();
    };

    let output = match Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,bit_rate",
            "-show_entries",
            "format=duration,bit_rate",
            "-of",
            "default=noprint_wrappers=1",
            path_str,
        ])
        .output()
    {
        Ok(output) => output,
        Err(_) => return VideoProbe::default(),
    };

    if !output.status.success() {
        return VideoProbe::default();
    }

    parse_ffprobe_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ffprobe_output(output: &str) -> VideoProbe {
    let mut probe = VideoProbe::default();
    let mut bitrates = Vec::new();

    for line in output.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "codec_name" if !value.is_empty() && probe.codec.is_none() => {
                probe.codec = Some(value.to_ascii_uppercase());
            }
            "duration" if probe.duration_ms.is_none() => {
                probe.duration_ms = parse_duration_ms(value);
            }
            "bit_rate" => {
                if let Some(bps) = parse_bitrate(value) {
                    bitrates.push(bps);
                }
            }
            _ => {}
        }
    }

    // ffprobe lists stream bit_rate before format bit_rate when both are present.
    probe.bitrate = bitrates.last().copied().or_else(|| bitrates.first().copied());
    probe
}

fn parse_duration_ms(value: &str) -> Option<u64> {
    let duration_secs: f64 = value.parse().ok()?;
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return None;
    }
    Some((duration_secs * 1000.0).round() as u64)
}

fn parse_bitrate(value: &str) -> Option<u32> {
    if value.eq_ignore_ascii_case("N/A") {
        return None;
    }
    value.parse::<u64>().ok().and_then(|bps| u32::try_from(bps).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_returns_none_for_missing_file() {
        assert_eq!(
            probe_video(Path::new("/no/such/video.mkv")),
            VideoProbe::default()
        );
    }

    #[test]
    fn parse_ffprobe_output_reads_codec_duration_and_format_bitrate() {
        let probe = parse_ffprobe_output(
            "codec_name=hevc\nbit_rate=8500000\nduration=5432.100000\nbit_rate=9200000\n",
        );

        assert_eq!(
            probe,
            VideoProbe {
                duration_ms: Some(5_432_100),
                bitrate: Some(9_200_000),
                codec: Some("HEVC".to_string()),
            }
        );
    }

    #[test]
    fn parse_ffprobe_output_falls_back_to_stream_bitrate() {
        let probe = parse_ffprobe_output("codec_name=h264\nbit_rate=4500000\n");

        assert_eq!(
            probe,
            VideoProbe {
                duration_ms: None,
                bitrate: Some(4_500_000),
                codec: Some("H264".to_string()),
            }
        );
    }

    #[test]
    fn parse_ffprobe_output_ignores_invalid_values() {
        let probe = parse_ffprobe_output(
            "codec_name=\nduration=not-a-number\nbit_rate=N/A\n",
        );

        assert_eq!(probe, VideoProbe::default());
    }
}
