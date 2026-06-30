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
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_track_count: Option<u32>,
    pub subtitle_track_count: Option<u32>,
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
            "stream=codec_name,bit_rate,width,height",
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

    let mut probe = parse_ffprobe_output(&String::from_utf8_lossy(&output.stdout));
    let (audio_count, subtitle_count) = probe_stream_type_counts(path_str);
    probe.audio_track_count = audio_count;
    probe.subtitle_track_count = subtitle_count;
    probe
}

fn probe_stream_type_counts(path_str: &str) -> (Option<u32>, Option<u32>) {
    let output = match Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path_str,
        ])
        .output()
    {
        Ok(output) => output,
        Err(_) => return (None, None),
    };

    if !output.status.success() {
        return (None, None);
    }

    parse_stream_type_counts(&String::from_utf8_lossy(&output.stdout))
}

fn parse_stream_type_counts(output: &str) -> (Option<u32>, Option<u32>) {
    let mut audio = 0u32;
    let mut subtitle = 0u32;

    for line in output.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match line {
            "audio" => audio += 1,
            "subtitle" => subtitle += 1,
            _ => {}
        }
    }

    (
        (audio > 0).then_some(audio),
        (subtitle > 0).then_some(subtitle),
    )
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
            "width" if probe.width.is_none() => {
                probe.width = parse_dimension(value);
            }
            "height" if probe.height.is_none() => {
                probe.height = parse_dimension(value);
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

fn parse_dimension(value: &str) -> Option<u32> {
    if value.eq_ignore_ascii_case("N/A") {
        return None;
    }
    value.parse::<u32>().ok().filter(|pixels| *pixels > 0)
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
            "codec_name=hevc\nwidth=3840\nheight=2160\nbit_rate=8500000\nduration=5432.100000\nbit_rate=9200000\n",
        );

        assert_eq!(
            probe,
            VideoProbe {
                duration_ms: Some(5_432_100),
                bitrate: Some(9_200_000),
                codec: Some("HEVC".to_string()),
                width: Some(3840),
                height: Some(2160),
                audio_track_count: None,
                subtitle_track_count: None,
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
                width: None,
                height: None,
                audio_track_count: None,
                subtitle_track_count: None,
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

    #[test]
    fn parse_stream_type_counts_reads_audio_and_subtitle_lines() {
        assert_eq!(
            parse_stream_type_counts("video\naudio\naudio\nsubtitle\n"),
            (Some(2), Some(1))
        );
    }

    #[test]
    fn parse_stream_type_counts_returns_none_when_no_matching_streams() {
        assert_eq!(parse_stream_type_counts("video\ndata\n"), (None, None));
    }
}
