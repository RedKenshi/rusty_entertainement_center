pub struct MediaState {
    pub path: PathBuf,
    pub favorite: bool,
    pub resume_position_ms: Option<u64>,
    pub last_watched_at: Option<SystemTime>,
}

pub struct Settings {
    pub last_opened_folder: Option<PathBuf>,
}

trait MediaStateRepository {
    async fn get(&self, path: &Path) -> Result<Option<MediaState>>;
    async fn save(&self, state: &MediaState) -> Result<()>;
}

trait SettingsRepository {
    async fn get_last_opened_folder(&self) -> Result<Option<PathBuf>>;
    async fn set_last_opened_folder(&self, path: &Path) -> Result<()>;
}