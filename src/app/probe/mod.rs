//! Video metadata probing via ffprobe (from ffmpeg).
//!
//! If ffprobe is not installed, all probe fields stay `None`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::Instant;

use crate::debug;

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

static FFPROBE_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// File extensions treated as video — must match the scanner (`browser`).
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "webm", "m4v", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "vob",
    "ogv", "3gp",
];

pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|video_ext| ext.eq_ignore_ascii_case(video_ext))
        })
        .unwrap_or(false)
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

pub fn probe_worker_count() -> usize {
    std::env::var("RUSTY_PROBE_WORKERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|count| *count > 0)
        .unwrap_or(if cfg!(feature = "kiosk") { 2 } else { 4 })
}

/// Probes metadata for many video paths in parallel. Non-video paths are skipped.
pub fn probe_videos_parallel(paths: &[PathBuf]) -> HashMap<PathBuf, VideoProbe> {
    let paths: Vec<PathBuf> = paths
        .iter()
        .filter(|path| is_video_path(path))
        .cloned()
        .collect();

    if paths.is_empty() || !ffprobe_available() {
        if paths.is_empty() {
            debug::scan("probe: no video paths");
        } else {
            debug::scan("probe: ffprobe not available");
        }
        return HashMap::new();
    }

    let workers = probe_worker_count().min(paths.len());
    let chunk_size = paths.len().div_ceil(workers);
    debug::scan(format!(
        "probe: dispatching {} file(s) to {workers} worker(s)",
        paths.len()
    ));
    let mut handles = Vec::new();

    for (worker_index, chunk) in paths.chunks(chunk_size.max(1)).enumerate() {
        let chunk = chunk.to_vec();
        handles.push(thread::spawn(move || {
            let mut results = HashMap::new();
            for path in chunk {
                if !is_video_path(&path) {
                    debug::scan(format!(
                        "  probe skip (worker {worker_index}): {} — not video",
                        path.display()
                    ));
                    continue;
                }
                debug::scan(format!(
                    "  probe start (worker {worker_index}): {}",
                    path.display()
                ));
                let started = Instant::now();
                let probe = probe_video(&path);
                debug::scan(format!(
                    "  probe done (worker {worker_index}): {} in {:?} duration={:?} codec={:?}",
                    path.display(),
                    started.elapsed(),
                    probe.duration_ms,
                    probe.codec
                ));
                results.insert(path, probe);
            }
            results
        }));
    }

    let mut merged = HashMap::new();
    for handle in handles {
        if let Ok(partial) = handle.join() {
            merged.extend(partial);
        }
    }
    debug::scan(format!("probe: merged {} result(s)", merged.len()));
    merged
}

/// Probes duration, codec, dimensions, bitrates, and stream counts in one ffprobe call.
pub fn probe_video(path: &Path) -> VideoProbe {
    if !is_video_path(path) {
        return VideoProbe::default();
    }

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
            "-show_entries",
            "stream=codec_name,codec_type,bit_rate,width,height",
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
    let mut audio = 0u32;
    let mut subtitle = 0u32;
    let mut current_stream_type: Option<&str> = None;

    for line in output.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "codec_type" => {
                current_stream_type = Some(value);
                match value {
                    "audio" => audio += 1,
                    "subtitle" => subtitle += 1,
                    _ => {}
                }
            }
            "codec_name" if !value.is_empty() => {
                if current_stream_type == Some("video") && probe.codec.is_none() {
                    probe.codec = Some(value.to_ascii_uppercase());
                }
            }
            "duration" if probe.duration_ms.is_none() => {
                probe.duration_ms = parse_duration_ms(value);
            }
            "bit_rate" => {
                if let Some(bps) = parse_bitrate(value) {
                    bitrates.push(bps);
                }
            }
            "width" if current_stream_type == Some("video") && probe.width.is_none() => {
                probe.width = parse_dimension(value);
            }
            "height" if current_stream_type == Some("video") && probe.height.is_none() => {
                probe.height = parse_dimension(value);
            }
            _ => {}
        }
    }

    probe.bitrate = bitrates.last().copied().or_else(|| bitrates.first().copied());
    probe.audio_track_count = (audio > 0).then_some(audio);
    probe.subtitle_track_count = (subtitle > 0).then_some(subtitle);
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
    fn is_video_path_matches_known_extensions() {
        assert!(is_video_path(Path::new("/a/file.mkv")));
        assert!(is_video_path(Path::new("/a/file.MP4")));
        assert!(!is_video_path(Path::new("/a/file.jpg")));
        assert!(!is_video_path(Path::new("/a/noext")));
    }

    #[test]
    fn probe_video_skips_non_video_paths() {
        assert_eq!(
            probe_video(Path::new("/a/photo.jpg")),
            VideoProbe::default()
        );
    }

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
            "codec_type=video\n\
             codec_name=hevc\n\
             width=3840\n\
             height=2160\n\
             bit_rate=8500000\n\
             codec_type=audio\n\
             codec_type=subtitle\n\
             duration=5432.100000\n\
             bit_rate=9200000\n",
        );

        assert_eq!(
            probe,
            VideoProbe {
                duration_ms: Some(5_432_100),
                bitrate: Some(9_200_000),
                codec: Some("HEVC".to_string()),
                width: Some(3840),
                height: Some(2160),
                audio_track_count: Some(1),
                subtitle_track_count: Some(1),
            }
        );
    }

    #[test]
    fn parse_ffprobe_output_falls_back_to_stream_bitrate() {
        let probe = parse_ffprobe_output(
            "codec_type=video\ncodec_name=h264\nbit_rate=4500000\n",
        );

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
            "codec_type=video\ncodec_name=\nduration=not-a-number\nbit_rate=N/A\n",
        );

        assert_eq!(probe, VideoProbe::default());
    }
}
