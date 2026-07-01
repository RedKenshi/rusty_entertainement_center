mod app;
mod db;
mod icons;
mod structs;
mod debug;
mod ui;
mod theme;
mod utils;
mod watch;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use app::{build_volume_library, wire_up, BrowsingState, WORKSPACE};
use db::{Database, SettingsRepository};
use slint::{BackendSelector, ComponentHandle};

fn database_path() -> PathBuf {
    PathBuf::from(WORKSPACE).join("library.db")
}

fn init_slint_backend() -> Result<(), slint::PlatformError> {
    #[cfg(feature = "kiosk")]
    {
        BackendSelector::new()
            .backend_name("linuxkms".into())
            .renderer_name("femtovg".into())
            .require_opengl_es()
            .select()
    }
    #[cfg(not(feature = "kiosk"))]
    {
        BackendSelector::new()
            .require_opengl()
            .select()
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let _ = dotenvy::dotenv();

    let dump_only = std::env::args().any(|arg| arg == "--dump-db");

    if dump_only {
        let database = Database::open(database_path()).expect("failed to open database");
        database
            .block_on(db::inspect::dump(database.pool()))
            .expect("failed to dump database");
        return Ok(());
    }

    debug::refresh(format!("starting app, workspace={WORKSPACE}"));

    init_slint_backend().expect(
        "OpenGL backend required for video playback \
         (on Pi kiosk builds use: cargo build --release --no-default-features --features kiosk)",
    );

    let database = Arc::new(
        Database::open(database_path()).expect("failed to open database"),
    );

    let tree = build_volume_library(WORKSPACE);
    if let Err(err) = database.block_on(db::reconcile::sync_tree(database.pool(), &tree)) {
        debug::db(format!("initial sync_tree failed: {err}"));
    }

    database.block_on(async {
        database.settings().get_last_opened_folder().await
    }).ok();

    let state = Rc::new(RefCell::new(BrowsingState::new(tree)));
    let window = ui::MainWindow::new()?;

    #[cfg(feature = "kiosk")]
    window.window().set_fullscreen(true);

    wire_up(&window, state, database);

    window.run()?;

    Ok(())
}
