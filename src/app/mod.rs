//! Application glue: connects Slint UI callbacks to domain state.
//!
//! `browsing/` holds navigation logic; `ui/` holds markup. This module owns
//! the wiring between them and is the right place for new handlers (help, player, …).

mod browser;
mod browsing;
mod probe;

pub use self::browser::{build_volume_library, WORKSPACE};
pub use self::browsing::BrowsingState;

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Global, ModelRc, Timer, TimerMode, VecModel};

use crate::db::{self, Database};
use crate::debug;
use crate::icons;
use crate::structs::FolderNode;
use crate::theme;
use crate::ui::{self, IconLoader, MainWindow};
use crate::watch;

/// Push the current browsing snapshot into Slint properties.
fn sync_window(window: &MainWindow, state: &BrowsingState) {
    window.set_tree(ModelRc::new(VecModel::from(
        state.visible_items().to_vec(),
    )));
    window.set_selected_index(state.selected as i32);
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

fn wire_navigation(window: &MainWindow, browsing_state: Rc<RefCell<BrowsingState>>) {
    // Each handler gets its own Weak + Rc clone; `browsing_state` is never moved.
    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    window.on_move_selection(move |delta| {
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
    window.on_open_selected(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        with_browsing(&state, &window, |browsing| {
            browsing.open_selected();
        });
    });

    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    window.on_navigate_back(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        with_browsing(&state, &window, |browsing| {
            browsing.go_back();
        });
    });
}

fn wire_help(window: &MainWindow) {
    let mut help_active: bool = false;
    
    // Weak ref: the window stores this callback, so a strong ref would leak.
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

fn wire_library_refresh(
    window: &MainWindow,
    browsing_state: Rc<RefCell<BrowsingState>>,
    database: Arc<Database>,
) {
    const QUIET_PERIOD: Duration = Duration::from_millis(800);

    debug::refresh(format!("workspace={WORKSPACE} debounce={QUIET_PERIOD:?}"));
    let change_events = watch::watch_workspace(WORKSPACE);
    let (tree_tx, tree_rx) = mpsc::channel::<Result<FolderNode, ()>>();
    let window_weak = window.as_weak();
    let state = Rc::clone(&browsing_state);
    let rescanning = Rc::new(AtomicBool::new(false));
    let mut dirty = false;
    let mut last_change: Option<Instant> = None;
    let mut rescan_pending_logged = false;
    let mut timer_started = false;

    let timer = Rc::new(Timer::default());
    let timer_keepalive = Rc::clone(&timer);
    timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        let _keepalive = timer_keepalive.clone();
        if !timer_started {
            timer_started = true;
            debug::refresh("debounce timer running");
        }

        let mut latest_tree = None;
        while let Ok(result) = tree_rx.try_recv() {
            rescanning.store(false, Ordering::Release);
            match result {
                Ok(tree) => latest_tree = Some(tree),
                Err(()) => {
                    debug::refresh("background rescan panicked, will retry");
                    dirty = true;
                    last_change = Some(Instant::now());
                    rescan_pending_logged = false;
                }
            }
        }
        if let Some(tree) = latest_tree {
            let volume_count = tree.subfolders.len();
            let file_count = tree.reduced_number_of_file;
            let visible_before = state.borrow().visible_items().len();
            let Some(window) = window_weak.upgrade() else {
                debug::refresh("rescan done but window gone, skipping UI apply");
                return;
            };
            with_browsing(&state, &window, |browsing| {
                browsing.reload_tree(tree);
            });
            let visible_after = state.borrow().visible_items().len();
            debug::refresh(format!(
                "tree applied: {volume_count} volumes, {file_count} files total, visible {visible_before} -> {visible_after}, stack={}",
                state.borrow().stack.len()
            ));
            rescan_pending_logged = false;
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

        let tree_tx = tree_tx.clone();
        let database = Arc::clone(&database);
        thread::spawn(move || {
            let started = Instant::now();
            debug::refresh("background rescan thread started");
            let result = catch_unwind(AssertUnwindSafe(|| {
                let tree = build_volume_library(WORKSPACE);
                reconcile_tree(&database, &tree);
                tree
            }))
            .map_err(|_| ());
            match &result {
                Ok(tree) => debug::refresh(format!(
                    "background rescan finished in {:?}: {} volumes, {} files",
                    started.elapsed(),
                    tree.subfolders.len(),
                    tree.reduced_number_of_file
                )),
                Err(()) => debug::refresh(format!(
                    "background rescan panicked after {:?}",
                    started.elapsed()
                )),
            }
            if tree_tx.send(result).is_err() {
                debug::refresh("failed to deliver rescan result (receiver dropped)");
            }
        });
    });
}

/// Register all UI callbacks and perform the initial window sync.
pub fn wire_up(
    window: &MainWindow,
    state: Rc<RefCell<BrowsingState>>,
    database: Arc<Database>,
) {
    wire_icons(window);
    sync_window(window, &state.borrow());
    wire_theme(window);
    wire_navigation(window, Rc::clone(&state));
    wire_help(window);
    wire_library_refresh(window, state, database);
}
