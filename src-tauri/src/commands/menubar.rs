//! The window's menu bar, and the key positions its shortcut labels need.
//!
//! Grouped by what they serve rather than by which platform module they call:
//! `keyboard_positions` reads Carbon and `set_menu` builds a Tauri menu, and
//! both exist so that one menu bar can be described by the frontend and drawn
//! by the platform.

#[cfg(target_os = "macos")]
use crate::keylayout;
use crate::menu;

/// What this keyboard prints on the keys a shortcut can name by position.
///
/// Keyed by `KeyboardEvent.code`. Empty on every platform but macOS, and empty
/// there too when the active input source carries no Unicode layout --- the
/// caller falls back to the character its binding declares, which is the label
/// the palette showed before this existed.
///
/// **On the main thread**, because HIToolbox aborts the process when the Text
/// Input Sources API is entered from two threads at once and says outright that
/// a UI application must call it from the main one. Same hop as `menu.rs`, for a
/// stricter reason: there it is AppKit's requirement, here it is a deliberate
/// `abort()` with a message naming the rule.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn keyboard_positions(
    app: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.blocking_send(keylayout::positions());
    })
    .map_err(|e| format!("could not reach the main thread to read the keyboard: {e}"))?;
    rx.recv()
        .await
        .ok_or_else(|| "the keyboard layout reader did not answer".to_string())
}

/// The non-macOS answer: no layout lookup, so every label stays its character.
///
/// An empty map rather than a refusal, for the same reason [`set_menu`] answers
/// `None`: there is nothing wrong on Windows, and the palette's own rendering is
/// what that platform has always shown.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn keyboard_positions() -> Result<std::collections::HashMap<String, String>, String> {
    Ok(std::collections::HashMap::new())
}

/// Installs the native menu bar from the layout the frontend holds.
///
/// Returns the event name a chosen item will arrive on, or `None` where there is
/// no menu bar --- which is every platform but macOS.
///
/// An answer rather than a refusal: nothing is wrong on Windows, the palette is
/// that platform's route, and an error there would put a red line in front of a
/// reader about a thing that was never meant to happen. So this is a capability
/// question, and the frontend stops sending enablement updates for a menu that
/// does not exist rather than pushing them into a silent no-op.
///
/// **The event name travels with the answer** for the reason `launch_open_event`
/// exists: a constant agreed in two languages fails by silence, and this one
/// would fail as a menu bar that is fully built, fully enabled, and does nothing
/// when clicked. One call carries both, so the name cannot be fetched for a menu
/// that was never installed.
///
/// The spec is built from the command registry; see `src/lib/menubar.ts`.
/// **One arm, with the platform question inside `menu.rs`.** There were two
/// until 2026-08-28, and the pair cost more than the duplication: the non-macOS
/// one took the payload as an unread `serde_json::Value`, because
/// `menu::SectionSpec` did not exist there --- so the contract between
/// `menubar.ts` and this command was type-checked on exactly one of the two
/// platforms tpdf ships. `menu::INSTALLS` carries the decision now, and the spec
/// is parsed into the same shape everywhere.
#[tauri::command]
pub async fn set_menu(
    app: tauri::AppHandle,
    sections: Vec<menu::SectionSpec>,
) -> Result<Option<&'static str>, String> {
    menu::install(&app, sections).await?;
    Ok(menu::INSTALLS.then_some(menu::RUN_EVENT))
}

/// Enables or disables menu items to match the commands' own guards.
///
/// Separate from [`set_menu`] because a rebuild per edit would rebuild the whole
/// bar several times a second while a reader works --- every rotation changes
/// whether Undo is live.
/// One arm, for [`set_menu`]'s reason.
#[tauri::command]
pub async fn set_menu_enabled(
    app: tauri::AppHandle,
    state: std::collections::HashMap<String, bool>,
) -> Result<(), String> {
    menu::set_enabled(&app, state).await
}
