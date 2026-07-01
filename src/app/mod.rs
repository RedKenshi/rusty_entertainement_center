//! Application glue: connects Slint UI callbacks to domain state.
//!
//! `browsing/` holds navigation logic; `ui/` holds markup. This module owns
//! the wiring between them and is the right place for new handlers (help, player, …).

mod browser;
mod browsing;
mod player;
mod probe;
mod volumes;

pub use self::browser::{build_volume_library, empty_library_root, WORKSPACE};
pub use self::browsing::{ActivateResult, BrowsingState};

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use slint::{ComponentHandle, Global, ModelRc, Timer, TimerMode, VecModel};

use crate::db::{self, normalize_path, Database, MediaState, MediaStateRepository};
use crate::debug;
use crate::icons;
use self::player::{PlaybackState, PlaybackStatus, PlayerEvent, PlayerHandle, wire_mpv_video_layer};
use crate::structs::FolderNode;
use crate::theme;
use crate::ui::{self, IconLoader, MainWindow};
use std::panic::{catch_unwind, AssertUnwindSafe};
use crate::utils::{format_playback_time, resume_position_to_store};
use self::browser::{probe_library, scan_volume_library};
use crate::watch;

fn sync_window(window: &MainWindow, state: &BrowsingState) {
    window.set_tree(ModelRc::new(VecModel::from(
        state.visible_items().to_vec(),
    )));
    window.set_selected_index(state.selected as i32);
}

fn load_resume_cache(database: &Database) -> HashMap<PathBuf, u64> {
    database
        .block_on(async { database.media().list_resume_positions().await })
        .unwrap_or_default()
}

fn apply_resume_cache(state: &Rc<RefCell<BrowsingState>>, cache: &HashMap<PathBuf, u64>) {
    state.borrow_mut().set_resume_positions(cache.clone());
}

fn resume_cache_key(path: &Path) -> PathBuf {
    PathBuf::from(normalize_path(path))
}

fn update_resume_cache_entry(
    cache: &Rc<RefCell<HashMap<PathBuf, u64>>>,
    path: &Path,
    position_ms: u64,
    duration_ms: Option<u64>,
) {
    let key = resume_cache_key(path);
    let mut cache = cache.borrow_mut();
    match resume_position_to_store(position_ms, duration_ms) {
        Some(ms) => {
            cache.insert(key, ms);
        }
        None => {
            cache.remove(&key);
        }
    }
}

fn clear_resume_in_db(database: &Database, path: &Path) {
    let path = path.to_path_buf();
    let pool = database.pool().clone();
    database.spawn(async move {
        let media = db::SqliteMediaRepository::new(pool);
        let mut state = media
            .get(&path)
            .await
            .ok()
            .flatten()
            .unwrap_or(MediaState {
                path: path.clone(),
                favorite: false,
                resume_position_ms: None,
                last_watched_at: None,
            });
        state.resume_position_ms = None;
        if let Err(err) = media.save(&state).await {
            debug::db(format!("resume clear failed: {err}"));
        }
    });
}

/// Run a browsing mutation, then refresh the window from the updated state.
fn with_browsing<F>(state: &Rc<RefCell<BrowsingState>>, window: &MainWindow, action: F)
where
    F: FnOnce(&mut BrowsingState),
{
    let mut browsing = state.borrow_mut();
    action(&mut browsing);
    sync_window(window, &browsing);
}

/// Resolve FaIcon names to SVG path data on demand (see `icons::load_icon`).
fn wire_icons(window: &MainWindow) {
    IconLoader::get(window).on_resolve(|name, style| {
        let icon = icons::load_icon(name.as_str(), style.as_str());
        ui::IconPaths {
            primary_path: icon.primary_path.into(),
            secondary_path: icon.secondary_path.into(),
            viewbox_width: icon.viewbox_width,
            viewbox_height: icon.viewbox_height,
        }
    });
}

fn wire_theme(window: &MainWindow) {
    let mut palette_index = 0usize;
    theme::apply_palette_by_index(window, palette_index);

    // Weak ref: the window stores this callback, so a strong ref would leak.
    let window_weak = window.as_weak();
    window.on_cycle_theme(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        palette_index = theme::next_palette_index(palette_index);
        theme::apply_palette_by_index(&window, palette_index);
    });
}

fn wire_navigation(
    window: &MainWindow,
    browsing_state: Rc<RefCell<BrowsingState>>,
    player: PlayerHandle,
    database: Arc<Database>,
    player_active: Rc<RefCell<bool>>,
) {
    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let player_active_move = Rc::clone(&player_active);
    window.on_move_selection(move |delta| {
        if *player_active_move.borrow() {
            return;
        }
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        with_browsing(&state, &window, |browsing| {
            if delta > 0 {
                browsing.go_down();
            } else if delta < 0 {
                browsing.go_up();
            }
        });
    });

    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let player = player.clone();
    let database = Arc::clone(&database);
    let player_active = Rc::clone(&player_active);
    let player_active_open = Rc::clone(&player_active);
    window.on_open_selected(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        if *player_active_open.borrow() {
            return;
        }

        let activate = {
            let mut browsing = state.borrow_mut();
            let result = browsing.activate_selected();
            if matches!(result, ActivateResult::OpenedFolder) {
                sync_window(&window, &browsing);
            }
            result
        };

        match activate {
            ActivateResult::PlayFile { path, name } => {
                let duration_ms = {
                    let browsing = state.borrow();
                    browsing.file_duration_ms(&path)
                };
                let resume_ms =
                    load_resume_position(&database, &path, duration_ms);
                debug::player(format!(
                    "open {} resume={resume_ms:?} duration={duration_ms:?}",
                    path.display()
                ));
                player.open(path, name, resume_ms, duration_ms);
                enter_player_mode(&window, &player_active_open);
            }
            ActivateResult::OpenedFolder | ActivateResult::None => {}
        }
    });

    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let player_active = Rc::clone(&player_active);
    window.on_navigate_back(move || {
        if *player_active.borrow() {
            return;
        }
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        with_browsing(&state, &window, |browsing| {
            browsing.go_back();
        });
    });
}

fn enter_player_mode(window: &MainWindow, player_active: &RefCell<bool>) {
    *player_active.borrow_mut() = true;
    window.set_player_active(true);
    window.set_help_visible(false);
    window.window().request_redraw();
}

fn hide_playback_toast(window: &MainWindow) {
    window.set_playback_toast_visible(false);
    window.set_playback_toast_text("".into());
}

fn show_playback_toast(
    window: &MainWindow,
    toast_timer: &Rc<RefCell<Option<Timer>>>,
    text: String,
) {
    if let Some(timer) = toast_timer.borrow_mut().take() {
        timer.stop();
    }

    window.set_playback_toast_text(text.into());
    window.set_playback_toast_visible(true);

    let window_weak = window.as_weak();
    let toast_timer_in_closure = Rc::clone(toast_timer);
    let hide_timer = Timer::default();
    hide_timer.start(TimerMode::SingleShot, Duration::from_millis(3000), move || {
        if let Some(window) = window_weak.upgrade() {
            hide_playback_toast(&window);
        }
        toast_timer_in_closure.borrow_mut().take();
    });
    *toast_timer.borrow_mut() = Some(hide_timer);
}

fn exit_player_mode(window: &MainWindow, player_active: &RefCell<bool>) {
    *player_active.borrow_mut() = false;
    window.set_player_active(false);
    window.set_playback_title("".into());
    window.set_playback_time_progress("0:00".into());
    window.set_playback_duration("0:00".into());
    window.set_playback_progress(0.0);
    window.set_playback_playing(false);
    window.set_playback_video(slint::Image::default());
    hide_playback_toast(window);
}

fn sync_playback_ui(window: &MainWindow, state: &PlaybackState) {
    window.set_playback_title(state.title.clone().into());
    window.set_playback_time_progress(format_playback_time(state.position_ms).into());
    window.set_playback_duration(
        format_playback_time(state.duration_ms.unwrap_or(0)).into(),
    );
    window.set_playback_progress(state.progress());
    window.set_playback_playing(state.status == PlaybackStatus::Playing);
}

fn load_resume_position(
    database: &Database,
    path: &Path,
    duration_ms: Option<u64>,
) -> Option<u64> {
    database
        .block_on(async { database.media().get(path).await })
        .ok()
        .flatten()
        .and_then(|state| state.resume_position_ms)
        .and_then(|position_ms| resume_position_to_store(position_ms, duration_ms))
}

fn persist_resume(
    database: &Database,
    path: &Path,
    position_ms: u64,
    duration_ms: Option<u64>,
    resume_cache: Option<&Rc<RefCell<HashMap<PathBuf, u64>>>>,
) {
    if let Some(cache) = resume_cache {
        update_resume_cache_entry(cache, path, position_ms, duration_ms);
    }
    let path = path.to_path_buf();
    let pool = database.pool().clone();
    let resume_position_ms = resume_position_to_store(position_ms, duration_ms);
    database.spawn(async move {
        let media = db::SqliteMediaRepository::new(pool);
        let mut state = media
            .get(&path)
            .await
            .ok()
            .flatten()
            .unwrap_or(MediaState {
                path: path.clone(),
                favorite: false,
                resume_position_ms: None,
                last_watched_at: None,
            });
        state.resume_position_ms = resume_position_ms;
        state.last_watched_at = Some(SystemTime::now());
        if let Err(err) = media.save(&state).await {
            debug::db(format!("resume save failed: {err}"));
        }
    });
}

fn wire_player(
    window: &MainWindow,
    database: Arc<Database>,
    player: PlayerHandle,
    event_rx: mpsc::Receiver<PlayerEvent>,
    player_active: Rc<RefCell<bool>>,
    browsing_state: Rc<RefCell<BrowsingState>>,
    resume_cache: Rc<RefCell<HashMap<PathBuf, u64>>>,
) {
    const RESUME_SAVE_INTERVAL: Duration = Duration::from_secs(15);

    let last_state = Rc::new(RefCell::new(PlaybackState::default()));
    let toast_timer = Rc::new(RefCell::new(None::<Timer>));

    let player_toggle = player.clone();
    window.on_playback_toggle_pause(move || {
        player_toggle.toggle_pause();
    });

    let player_seek = player.clone();
    window.on_playback_seek_backward(move |seek_ms| {
        player_seek.seek_delta(-(seek_ms as i64));
    });

    let player_seek = player.clone();
    window.on_playback_seek_forward(move |seek_ms| {
        player_seek.seek_delta(seek_ms as i64);
    });

    let player_cycle_audio = player.clone();
    window.on_playback_cycle_audio(move || {
        player_cycle_audio.cycle_audio_track();
    });

    let player_cycle_subtitle = player.clone();
    window.on_playback_cycle_subtitle(move || {
        player_cycle_subtitle.cycle_subtitle_track();
    });

    let window_weak_stop = window.as_weak();
    let player_stop = player.clone();
    let database_stop = Arc::clone(&database);
    let player_active_stop = Rc::clone(&player_active);
    let last_state_stop = Rc::clone(&last_state);
    let resume_cache_stop = Rc::clone(&resume_cache);
    let browsing_stop = Rc::clone(&browsing_state);
    window.on_playback_stop(move || {
        let Some(window) = window_weak_stop.upgrade() else {
            return;
        };
        let snapshot = last_state_stop.borrow().clone();
        if let Some(path) = snapshot.path.as_ref() {
            persist_resume(
                &database_stop,
                path,
                snapshot.position_ms,
                snapshot.duration_ms,
                Some(&resume_cache_stop),
            );
        }
        player_stop.stop();
        exit_player_mode(&window, &player_active_stop);
        apply_resume_cache(&browsing_stop, &resume_cache_stop.borrow());
        sync_window(&window, &browsing_stop.borrow());
    });

    let window_weak = window.as_weak();
    let database_timer = Arc::clone(&database);
    let player_active_timer = Rc::clone(&player_active);
    let resume_cache_timer = Rc::clone(&resume_cache);
    let browsing_timer = Rc::clone(&browsing_state);
    let mut last_status = PlaybackStatus::Stopped;
    let mut last_resume_save = Instant::now();

    let event_rx = event_rx;
    let last_state_timer = Rc::clone(&last_state);
    let toast_timer_events = Rc::clone(&toast_timer);
    let timer = Rc::new(Timer::default());
    let timer_keepalive = Rc::clone(&timer);
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let _keepalive = timer_keepalive.clone();
        let Some(window) = window_weak.upgrade() else {
            return;
        };

        while let Ok(event) = event_rx.try_recv() {
            match event {
                PlayerEvent::State(state) => {
                    if *player_active_timer.borrow() {
                        if last_status == PlaybackStatus::Playing
                            && state.status == PlaybackStatus::Paused
                        {
                            if let Some(path) = state.path.as_ref() {
                                persist_resume(
                                    &database_timer,
                                    path,
                                    state.position_ms,
                                    state.duration_ms,
                                    Some(&resume_cache_timer),
                                );
                            }
                        }
                        last_status = state.status;
                    }
                    *last_state_timer.borrow_mut() = state;
                }
                PlayerEvent::Stopped => {
                    let snapshot = last_state_timer.borrow().clone();
                    if let Some(path) = snapshot.path.as_ref() {
                        persist_resume(
                            &database_timer,
                            path,
                            snapshot.position_ms,
                            snapshot.duration_ms,
                            Some(&resume_cache_timer),
                        );
                    }
                    if let Some(timer) = toast_timer_events.borrow_mut().take() {
                        timer.stop();
                    }
                    exit_player_mode(&window, &player_active_timer);
                    apply_resume_cache(&browsing_timer, &resume_cache_timer.borrow());
                    sync_window(&window, &browsing_timer.borrow());
                    *last_state_timer.borrow_mut() = PlaybackState::default();
                    last_status = PlaybackStatus::Stopped;
                }
                PlayerEvent::TrackToast(text) => {
                    show_playback_toast(&window, &toast_timer_events, text);
                }
            }
        }

        if !*player_active_timer.borrow() {
            return;
        }

        let snapshot = last_state_timer.borrow().clone();
        sync_playback_ui(&window, &snapshot);

        if snapshot.status == PlaybackStatus::Playing
            && last_resume_save.elapsed() >= RESUME_SAVE_INTERVAL
        {
            if let Some(path) = snapshot.path.as_ref() {
                persist_resume(
                    &database_timer,
                    path,
                    snapshot.position_ms,
                    snapshot.duration_ms,
                    Some(&resume_cache_timer),
                );
            }
            last_resume_save = Instant::now();
        }
    });
}

fn wire_resume_clear(
    window: &MainWindow,
    browsing_state: Rc<RefCell<BrowsingState>>,
    database: Arc<Database>,
    resume_cache: Rc<RefCell<HashMap<PathBuf, u64>>>,
    player_active: Rc<RefCell<bool>>,
) {
    const HOLD_DURATION: Duration = Duration::from_secs(3);

    let hold_generation = Rc::new(RefCell::new(0u64));
    let hold_timer = Rc::new(RefCell::new(None::<Timer>));
    let hold_active = Rc::new(RefCell::new(false));

    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let database_start = Arc::clone(&database);
    let resume_cache_start = Rc::clone(&resume_cache);
    let player_active_start = Rc::clone(&player_active);
    let hold_generation_start = Rc::clone(&hold_generation);
    let hold_timer_start = Rc::clone(&hold_timer);
    let hold_active_start = Rc::clone(&hold_active);
    window.on_resume_clear_hold_started(move || {
        if *player_active_start.borrow() {
            debug::browse("resume clear hold: ignored (player active)");
            return;
        }

        if *hold_active_start.borrow() {
            debug::browse("resume clear hold: ignored (key repeat)");
            return;
        }

        let path = {
            let browsing = state.borrow();
            let Some(path) = browsing.selected_file_path() else {
                debug::browse("resume clear hold: ignored (not a file selection)");
                return;
            };
            if !browsing.selected_file_has_resume() {
                debug::browse(format!(
                    "resume clear hold: ignored (no resume for {})",
                    path.display()
                ));
                return;
            }
            path
        };

        *hold_active_start.borrow_mut() = true;
        *hold_generation_start.borrow_mut() += 1;
        let generation = *hold_generation_start.borrow();
        hold_timer_start.borrow_mut().take();

        debug::browse(format!(
            "resume clear hold: started for {} ({HOLD_DURATION:?})",
            path.display()
        ));

        let window_weak = window_weak.clone();
        let state = Rc::clone(&state);
        let database = Arc::clone(&database_start);
        let resume_cache = Rc::clone(&resume_cache_start);
        let hold_generation = Rc::clone(&hold_generation_start);
        let hold_active = Rc::clone(&hold_active_start);
        let cache_key = resume_cache_key(&path);
        let timer = Timer::default();
        timer.start(TimerMode::SingleShot, HOLD_DURATION, move || {
            if generation != *hold_generation.borrow() {
                debug::browse("resume clear hold: timer fired but generation mismatch");
                return;
            }

            *hold_active.borrow_mut() = false;

            let Some(window) = window_weak.upgrade() else {
                debug::browse("resume clear hold: timer fired but window gone");
                return;
            };

            resume_cache.borrow_mut().remove(&cache_key);
            clear_resume_in_db(&database, &path);
            apply_resume_cache(&state, &resume_cache.borrow());
            sync_window(&window, &state.borrow());
            debug::browse(format!("resume clear hold: cleared {}", path.display()));
        });
        *hold_timer_start.borrow_mut() = Some(timer);
    });

    let hold_generation_end = Rc::clone(&hold_generation);
    let hold_timer_end = Rc::clone(&hold_timer);
    let hold_active_end = Rc::clone(&hold_active);
    window.on_resume_clear_hold_ended(move || {
        let was_active = *hold_active_end.borrow();
        *hold_active_end.borrow_mut() = false;
        *hold_generation_end.borrow_mut() += 1;
        hold_timer_end.borrow_mut().take();
        if was_active {
            debug::browse("resume clear hold: released before timeout");
        }
    });
}

fn wire_help(window: &MainWindow) {
    let mut help_active: bool = false;

    let window_weak = window.as_weak();
    window.on_toggle_help(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        help_active = !help_active;
        window.set_help_visible(help_active);
    });
}

fn reconcile_tree(database: &Database, tree: &FolderNode) {
    if let Err(err) = database.block_on(db::reconcile::sync_tree(database.pool(), tree)) {
        debug::db(format!("sync_tree failed: {err}"));
    }
}

fn spawn_library_build(tree_tx: mpsc::Sender<Result<FolderNode, ()>>, database: Arc<Database>) {
    thread::spawn(move || {
        let started = Instant::now();
        debug::scan("library build started");
        let build_result = catch_unwind(AssertUnwindSafe(|| {
            let mut tree = scan_volume_library(WORKSPACE);
            let _ = tree_tx.send(Ok(tree.clone()));

            probe_library(&mut tree);
            reconcile_tree(&database, &tree);
            tree
        }))
        .map_err(|_| ());

        match &build_result {
            Ok(tree) => debug::scan(format!(
                "library build finished in {:?}: {} volume(s), {} file(s)",
                started.elapsed(),
                tree.subfolders.len(),
                tree.reduced_number_of_file
            )),
            Err(()) => debug::scan(format!(
                "library build panicked after {:?}",
                started.elapsed()
            )),
        }

        match build_result {
            Ok(tree) => {
                let _ = tree_tx.send(Ok(tree));
            }
            Err(()) => {
                let _ = tree_tx.send(Err(()));
            }
        }
    });
}

fn wire_library_refresh(
    window: &MainWindow,
    browsing_state: Rc<RefCell<BrowsingState>>,
    database: Arc<Database>,
) {
    const QUIET_PERIOD: Duration = Duration::from_millis(800);

    let watch_roots = volumes::watch_roots(WORKSPACE);
    debug::refresh(format!(
        "workspace={WORKSPACE} fake_volumes={} watch_roots={} debounce={QUIET_PERIOD:?}",
        volumes::use_fake_volumes(),
        watch_roots
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(",")
    ));
    let change_events = watch::watch_paths(&watch_roots);
    let (tree_tx, tree_rx) = mpsc::channel::<Result<FolderNode, ()>>();
    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let rescanning = Rc::new(AtomicBool::new(false));
    let mut dirty = false;
    let mut last_change: Option<Instant> = None;
    let mut rescan_pending_logged = false;
    let mut timer_started = false;

    rescanning.store(true, Ordering::Release);
    debug::scan("starting initial library build");
    spawn_library_build(tree_tx.clone(), Arc::clone(&database));

    let timer = Rc::new(Timer::default());
    let timer_keepalive = Rc::clone(&timer);
    timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        let _keepalive = timer_keepalive.clone();
        if !timer_started {
            timer_started = true;
            debug::refresh("debounce timer running");
        }

        while let Ok(result) = tree_rx.try_recv() {
            rescanning.store(false, Ordering::Release);
            match result {
                Ok(tree) => {
                    let volume_count = tree.subfolders.len();
                    let file_count = tree.reduced_number_of_file;
                    let visible_before = state.borrow().visible_items().len();
                    let Some(window) = window_weak.upgrade() else {
                        debug::refresh("rescan done but window gone, skipping UI apply");
                        break;
                    };
                    with_browsing(&state, &window, |browsing| {
                        browsing.reload_tree(tree);
                    });
                    let visible_after = state.borrow().visible_items().len();
                    debug::scan(format!(
                        "tree applied to UI: {volume_count} volume(s), {file_count} file(s), visible {visible_before} -> {visible_after}"
                    ));
                    rescan_pending_logged = false;
                }
                Err(()) => {
                    debug::refresh("background rescan panicked, will retry");
                    dirty = true;
                    last_change = Some(Instant::now());
                    rescan_pending_logged = false;
                }
            }
        }

        while change_events.try_recv().is_ok() {
            dirty = true;
            last_change = Some(Instant::now());
            rescan_pending_logged = false;
        }

        if !dirty || rescanning.load(Ordering::Acquire) {
            return;
        }

        let Some(at) = last_change else {
            return;
        };
        if at.elapsed() < QUIET_PERIOD {
            return;
        }

        if !rescan_pending_logged {
            debug::refresh(format!(
                "quiet for {:?}, starting rescan",
                at.elapsed()
            ));
            rescan_pending_logged = true;
        }

        dirty = false;
        rescanning.store(true, Ordering::Release);
        spawn_library_build(tree_tx.clone(), Arc::clone(&database));
    });
}

/// Register all UI callbacks and perform the initial window sync.
pub fn wire_up(
    window: &MainWindow,
    state: Rc<RefCell<BrowsingState>>,
    database: Arc<Database>,
) {
    let (player, command_rx, event_tx, event_rx) = PlayerHandle::spawn();
    let player_active = Rc::new(RefCell::new(false));
    let resume_cache = Rc::new(RefCell::new(load_resume_cache(&database)));

    wire_icons(window);
    apply_resume_cache(&state, &resume_cache.borrow());
    sync_window(window, &state.borrow());
    wire_theme(window);
    wire_navigation(
        window,
        Rc::clone(&state),
        player.clone(),
        Arc::clone(&database),
        Rc::clone(&player_active),
    );
    wire_mpv_video_layer(
        window,
        command_rx,
        event_tx,
        Rc::clone(&player_active),
    );
    wire_player(
        window,
        Arc::clone(&database),
        player,
        event_rx,
        Rc::clone(&player_active),
        Rc::clone(&state),
        Rc::clone(&resume_cache),
    );
    wire_resume_clear(
        window,
        Rc::clone(&state),
        Arc::clone(&database),
        Rc::clone(&resume_cache),
        Rc::clone(&player_active),
    );
    wire_help(window);
    wire_library_refresh(window, state, database);
}
