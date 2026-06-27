mod probe;

mod icons;

mod structs;

mod browser;
use browser::*;

mod browsing;
use browsing::BrowsingState;

#[cfg(debug_assertions)]
mod debug;

mod ui;
use ui::*;

mod theme;

mod utils;

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ModelRc, VecModel};

fn sync_window(window: &MainWindow, state: &BrowsingState) {
    window.set_tree(ModelRc::new(VecModel::from(
        state.visible_items().to_vec(),
    )));
    window.set_selected_index(state.selected as i32);
}

fn with_browsing<F>(state: &Rc<RefCell<BrowsingState>>, window: &MainWindow, action: F)
where
    F: FnOnce(&mut BrowsingState),
{
    let mut browsing = state.borrow_mut();
    action(&mut browsing);
    sync_window(window, &browsing);
}

fn main() -> Result<(), slint::PlatformError> {
    let tree = build_volume_library(".");
    let state = Rc::new(RefCell::new(BrowsingState::new(tree)));
    let window = ui::MainWindow::new()?;

    ui::IconLoader::get(&window).on_resolve(|name, style| {
        let icon = icons::load_icon(name.as_str(), style.as_str());
        ui::IconPaths {
            primary_path: icon.primary_path.into(),
            secondary_path: icon.secondary_path.into(),
            viewbox_width: icon.viewbox_width,
            viewbox_height: icon.viewbox_height,
        }
    });

/*     #[cfg(debug_assertions)]
    debug::print_folder(&state.borrow().tree); */

    sync_window(&window, &state.borrow());

    let mut palette_index = 0usize;
    theme::apply_palette_by_index(&window, palette_index);

    {
        let window_weak = window.as_weak();
        window.on_cycle_theme(move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            palette_index = theme::next_palette_index(palette_index);
            theme::apply_palette_by_index(&window, palette_index);
        });
    }

    {
        let state = state.clone();
        let window_weak = window.as_weak();
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
    }

    {
        let state = state.clone();
        let window_weak = window.as_weak();
        window.on_open_selected(move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            with_browsing(&state, &window, |browsing| {
                browsing.open_selected();
            });
        });
    }

    {
        let state = state.clone();
        let window_weak = window.as_weak();
        window.on_navigate_back(move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            with_browsing(&state, &window, |browsing| {
                browsing.go_back();
            });
        });
    }

    window.run()?;

    Ok(())
}
