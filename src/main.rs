mod app;
mod config;
mod kubernetes;
mod session;
mod terminal;
mod ui;

use gpui::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::app::AppState;
use crate::ui::{open_main_window, QuitConfirmDialog, SessionDialog, SsmSessionDialog};

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting RedPill");

    // Initialize the gpui application
    Application::new()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx: &mut App| {
        // Set up application menu (macOS)
        #[cfg(target_os = "macos")]
        {
            cx.set_menus(vec![
                Menu {
                    name: "RedPill".into(),
                    items: vec![
                        MenuItem::action("About RedPill", About),
                        MenuItem::separator(),
                        MenuItem::action("Settings...", ShowSettings),
                        MenuItem::separator(),
                        MenuItem::action("Quit", Quit),
                    ],
                },
                Menu {
                    name: "File".into(),
                    items: vec![
                        MenuItem::action("New Terminal", NewTerminal),
                        MenuItem::action("New SSH Session...", NewSshSession),
                        MenuItem::action("New SSM Session...", NewSsmSession),
                        MenuItem::separator(),
                        MenuItem::action("Close Tab", CloseTab),
                    ],
                },
                Menu {
                    name: "Edit".into(),
                    items: vec![
                        MenuItem::action("Copy", Copy),
                        MenuItem::action("Paste", Paste),
                        MenuItem::separator(),
                        MenuItem::action("Select All", SelectAll),
                    ],
                },
                Menu {
                    name: "View".into(),
                    items: vec![
                        MenuItem::action("Toggle Session Tree", ToggleSessionTree),
                        MenuItem::action("Show Scrollbar", ToggleScrollbar),
                        MenuItem::separator(),
                        MenuItem::action("Zoom In", ZoomIn),
                        MenuItem::action("Zoom Out", ZoomOut),
                        MenuItem::action("Reset Zoom", ZoomReset),
                        MenuItem::separator(),
                        MenuItem::action("Theme: Default", SchemeDefault),
                        MenuItem::action("Theme: Light", SchemeLight),
                        MenuItem::action("Theme: Matrix", SchemeMatrix),
                    ],
                },
            ]);
        }

        // Register global actions
        cx.on_action(|_: &Quit, cx| {
            // Check for active SSH connections before quitting
            let ssh_count = if let Some(state) = cx.try_global::<AppState>() {
                state.app.lock().active_ssh_connection_count()
            } else {
                0
            };

            if ssh_count > 0 {
                // Show confirmation dialog
                QuitConfirmDialog::open(ssh_count, cx);
            } else {
                // No active connections, quit immediately
                cx.quit();
            }
        });

        cx.on_action(|_: &About, _cx| {
            tracing::info!("RedPill - SSH Terminal Manager v{}", env!("CARGO_PKG_VERSION"));
        });

        // NewTerminal - open a new local terminal
        cx.on_action(|_: &NewTerminal, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                if let Err(e) = state.app.lock().open_local_terminal() {
                    tracing::error!("Failed to open terminal: {}", e);
                }
            }
            cx.refresh_windows();
        });

        // NewSshSession - open the SSH session dialog
        cx.on_action(|_: &NewSshSession, cx| {
            SessionDialog::open_new(cx);
        });

        // NewSsmSession - open the SSM session dialog
        cx.on_action(|_: &NewSsmSession, cx| {
            SsmSessionDialog::open_new(cx);
        });

        // CloseTab - close the active tab
        cx.on_action(|_: &CloseTab, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                if let Some(tab) = app.active_tab() {
                    let tab_id = tab.id;
                    app.close_tab(tab_id);
                }
            }
            cx.refresh_windows();
        });

        // ToggleSessionTree - toggle session tree visibility
        cx.on_action(|_: &ToggleSessionTree, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                state.app.lock().toggle_session_tree();
            }
            cx.refresh_windows();
        });

        // ToggleScrollbar - toggle scrollbar visibility
        cx.on_action(|_: &ToggleScrollbar, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.show_scrollbar = !app.config.show_scrollbar;
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // ZoomIn - increase font size
        cx.on_action(|_: &ZoomIn, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.zoom_in();
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // ZoomOut - decrease font size
        cx.on_action(|_: &ZoomOut, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.zoom_out();
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // ZoomReset - reset font size to default
        cx.on_action(|_: &ZoomReset, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.zoom_reset();
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // SchemeDefault - switch to default dark theme
        cx.on_action(|_: &SchemeDefault, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.set_scheme("default");
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // SchemeLight - switch to light theme
        cx.on_action(|_: &SchemeLight, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.set_scheme("light");
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // SchemeMatrix - switch to matrix theme
        cx.on_action(|_: &SchemeMatrix, cx| {
            if let Some(state) = cx.try_global::<AppState>() {
                let mut app = state.app.lock();
                app.config.appearance.set_scheme("matrix");
                let _ = app.config.save();
            }
            cx.refresh_windows();
        });

        // ShowSettings - placeholder for settings dialog
        cx.on_action(|_: &ShowSettings, _cx| {
            tracing::info!("Settings dialog not yet implemented");
        });

        // Copy - handled by MainWindow which has access to terminal views
        // Paste - handled by MainWindow which has access to terminal views
        // SelectAll - handled by MainWindow which has access to terminal views

        // Open the main window and activate the app
        open_main_window(cx);
        cx.activate(true);
    });
}

// Action definitions
actions!(
    redpill,
    [
        About,
        Quit,
        ShowSettings,
        NewTerminal,
        NewSshSession,
        NewSsmSession,
        CloseTab,
        Copy,
        Paste,
        SelectAll,
        ToggleSessionTree,
        ToggleScrollbar,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        SchemeDefault,
        SchemeLight,
        SchemeMatrix,
    ]
);
