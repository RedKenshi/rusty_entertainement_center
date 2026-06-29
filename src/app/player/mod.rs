mod mpv_gl;
mod state;

pub use state::{PlaybackState, PlaybackStatus};

use std::path::PathBuf;
use std::sync::mpsc;

pub enum PlayerCommand {
    Open {
        path: PathBuf,
        title: String,
        resume_ms: Option<u64>,
        duration_ms: Option<u64>,
    },
    Play,
    Pause,
    TogglePause,
    Stop,
    SeekTo(u64),
    SeekDelta(i64),
    CycleAudioTrack,
    CycleSubtitleTrack,
    Shutdown,
}

pub enum PlayerEvent {
    State(PlaybackState),
    Stopped,
}

/// UI-facing handle: enqueue commands to the mpv render loop.
#[derive(Clone)]
pub struct PlayerHandle {
    command_tx: mpsc::Sender<PlayerCommand>,
}

impl PlayerHandle {
    pub fn spawn() -> (
        Self,
        mpsc::Receiver<PlayerCommand>,
        mpsc::Sender<PlayerEvent>,
        mpsc::Receiver<PlayerEvent>,
    ) {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        (Self { command_tx }, command_rx, event_tx, event_rx)
    }

    pub fn open(
        &self,
        path: PathBuf,
        title: String,
        resume_ms: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        let _ = self.command_tx.send(PlayerCommand::Open {
            path,
            title,
            resume_ms,
            duration_ms,
        });
    }

    pub fn toggle_pause(&self) {
        let _ = self.command_tx.send(PlayerCommand::TogglePause);
    }

    pub fn stop(&self) {
        let _ = self.command_tx.send(PlayerCommand::Stop);
    }

    pub fn seek_delta(&self, delta_ms: i64) {
        let _ = self.command_tx.send(PlayerCommand::SeekDelta(delta_ms));
    }
}

pub use mpv_gl::wire_mpv_video_layer;
