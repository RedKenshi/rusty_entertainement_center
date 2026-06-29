use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackInfo {
    pub index: u32,
    pub label: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub path: Option<PathBuf>,
    pub title: String,
    pub status: PlaybackStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub audio_tracks: Vec<TrackInfo>,
    pub subtitle_tracks: Vec<TrackInfo>,
    pub selected_audio: u32,
    pub selected_subtitle: Option<u32>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            path: None,
            title: String::new(),
            status: PlaybackStatus::Stopped,
            position_ms: 0,
            duration_ms: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            selected_audio: 0,
            selected_subtitle: None,
        }
    }
}

impl PlaybackState {
    pub fn progress(&self) -> f32 {
        let Some(duration) = self.duration_ms.filter(|ms| *ms > 0) else {
            return 0.0;
        };
        (self.position_ms as f32 / duration as f32).clamp(0.0, 1.0)
    }

    pub fn apply_seek_delta(&mut self, delta_ms: i64) {
        let duration = self.duration_ms.unwrap_or(self.position_ms);
        let target = (self.position_ms as i64 + delta_ms).clamp(0, duration as i64);
        self.position_ms = target as u64;
    }

    pub fn apply_seek_to(&mut self, position_ms: u64) {
        let max = self.duration_ms.unwrap_or(position_ms);
        self.position_ms = position_ms.min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_delta_clamps_at_zero_and_duration() {
        let mut state = PlaybackState {
            duration_ms: Some(60_000),
            position_ms: 5_000,
            ..Default::default()
        };
        state.apply_seek_delta(-10_000);
        assert_eq!(state.position_ms, 0);
        state.apply_seek_delta(100_000);
        assert_eq!(state.position_ms, 60_000);
    }
}
