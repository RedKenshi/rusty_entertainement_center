mod app;
mod probe;

mod icons;

mod structs;

mod browser;
use browser::*;

mod browsing;
use browsing::BrowsingState;

mod debug;

mod ui;

mod theme;

mod utils;

mod watch;

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    debug::refresh(format!("starting app, workspace={WORKSPACE}"));
    let tree = build_volume_library(WORKSPACE);
    let state = Rc::new(RefCell::new(BrowsingState::new(tree)));
    let window = ui::MainWindow::new()?;

/*     #[cfg(debug_assertions)]
    debug::print_folder(&state.borrow().tree); */

    app::wire_up(&window, state);

    window.run()?;

    Ok(())
}
