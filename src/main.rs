mod app;

mod icons;

mod structs;

mod debug;

mod ui;

mod theme;

mod utils;

mod watch;

use std::cell::RefCell;
use std::rc::Rc;

use app::{build_volume_library, wire_up, BrowsingState, WORKSPACE};
use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    debug::refresh(format!("starting app, workspace={WORKSPACE}"));
    let tree = build_volume_library(WORKSPACE);
    let state = Rc::new(RefCell::new(BrowsingState::new(tree)));
    let window = ui::MainWindow::new()?;

/*     #[cfg(debug_assertions)]
    debug::print_folder(&state.borrow().tree); */

    wire_up(&window, state);

    window.run()?;

    Ok(())
}
