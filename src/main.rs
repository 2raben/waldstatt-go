//! Waldstatt Go: rivals fell their way through the forest round Einsiedeln and
//! win each other's Waldleute by enclosing them, as in Go.

mod ai;
#[cfg(not(target_arch = "wasm32"))]
mod arena;
mod art;
mod camera;
mod ground;
mod hud;
mod input;
mod markers;
mod people;
mod rules;
mod scenery;
mod screens;
mod style;
mod view;

#[cfg(test)]
mod tests;

use bevy::prelude::*;

fn main() -> AppExit {
    // `waldstatt-go arena` plays computer rivals against each other, headless.
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::args().nth(1).as_deref() == Some("arena") {
        return arena::run(std::env::args().skip(2));
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window()),
            ..default()
        }))
        .add_plugins((
            // The rules and the computer rivals, headless (ADR 0002).
            rules::RulesPlugin,
            ai::ComputerRivalsPlugin,
            // Presentation.
            style::StylePlugin,
            art::ArtPlugin,
            view::ViewPlugin,
            screens::ScreensPlugin,
            input::InputPlugin,
            camera::CameraPlugin,
            ground::GroundPlugin,
            markers::MarkersPlugin,
            scenery::SceneryPlugin,
            people::PeoplePlugin,
            hud::HudPlugin,
        ))
        .run()
}

/// A 1280 × 800 window on the desktop.
#[cfg(not(target_arch = "wasm32"))]
fn window() -> Window {
    Window {
        title: "Waldstatt Go".into(),
        resolution: (1280, 800).into(),
        ..default()
    }
}

/// In the browser, the canvas of `web/index.html`, sized by the page. Bevy
/// keeps the browser's own handling of keys away from it, so Tab and Space
/// play instead of leaving or scrolling the page.
#[cfg(target_arch = "wasm32")]
fn window() -> Window {
    Window {
        title: "Waldstatt Go".into(),
        canvas: Some("#waldstatt-go".into()),
        fit_canvas_to_parent: true,
        ..default()
    }
}
