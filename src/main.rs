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
use slint::ComponentHandle;

fn database_path() -> PathBuf {
    PathBuf::from(WORKSPACE).join("library.db")
}

fn main() -> Result<(), slint::PlatformError> {
    let dump_only = std::env::args().any(|arg| arg == "--dump-db");

    if dump_only {
        let database = Database::open(database_path()).expect("failed to open database");
        database
            .block_on(db::inspect::dump(database.pool()))
            .expect("failed to dump database");
        return Ok(());
    }

    debug::refresh(format!("starting app, workspace={WORKSPACE}"));

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

    wire_up(&window, state, database);

    window.run()?;

    Ok(())
}
