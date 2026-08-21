//! The native menu bar, built from a specification the frontend sends.
//!
//! **The menu is generated, not written.** `src/lib/menubar.ts` reads the same
//! `CommandRegistry` the palette reads and sends the result here; this module
//! turns it into AppKit menus and turns a click back into the command's id. No
//! command title, shortcut or guard is restated in Rust, because a second
//! statement of any of them is a second thing to keep right --- the same
//! argument that put every keyboard binding in one table after ⌘O had been
//! advertised in the palette while reaching no handler at all.
//!
//! **Why the frontend owns the list.** The alternative is a layout table here,
//! which would drift from the registry silently: a command added in TypeScript
//! would simply not appear, and nothing would say so. Sending the spec costs one
//! call after the palette's commands are registered, and it lets the frontend's
//! own test assert that every command has a home. The menu therefore appears a
//! beat after launch rather than at process start, which is the honest price and
//! is invisible next to the ~250 ms shell floor `docs/PLAN.md` §4 measures.
//!
//! **Enablement is updated in place, not rebuilt.** A menu rebuild per edit ---
//! every rotate changes whether Undo is live --- would rebuild the whole bar
//! several times a second while a reader works. [`set_enabled`] walks a map of
//! the items built by [`install`] instead.
//!
//! **macOS only.** The whole module is behind `#[cfg(target_os = "macos")]` at
//! its call site: there the bar lives outside the window and costs the reader
//! nothing, which is why its emptiness was a defect worth fixing. On Windows a
//! menu bar is chrome inside the window, and this application exists partly
//! because the alternatives put a ribbon there.
//!
//! **Every menu call happens on the main thread.** AppKit requires it, and a
//! Tauri command does not run there. [`install`] and [`set_enabled`] are the two
//! entry points and both hop across; a caller that forgets is not a warning on
//! this platform, it is a crash inside AppKit with our own frame nowhere in it.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The event a chosen menu item becomes, carrying the command's id.
///
/// Named like the launch event next door, and for the same reason: the string is
/// matched in TypeScript, so it is a shared constant rather than a literal
/// written twice.
pub const RUN_EVENT: &str = "menu://run";

/// One entry in a menu.
///
/// Tagged rather than an `Option`, so that "a separator" and "an item that could
/// not be built" cannot arrive as the same value.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ItemSpec {
    Separator,
    Command {
        id: String,
        title: String,
        /// Absent for a command whose chord something else on the platform
        /// claims --- `menubar.ts` has the two families and why.
        accelerator: Option<String>,
        enabled: bool,
    },
}

/// One menu in the bar.
#[derive(Deserialize, Debug, Clone)]
pub struct SectionSpec {
    pub title: String,
    /// Whether this is the application menu, whose predefined items are the
    /// platform's rather than ours.
    pub app: bool,
    pub items: Vec<ItemSpec>,
}

/// The built items, by command id, so enablement can be changed without a
/// rebuild.
pub struct MenuItems<R: Runtime> {
    items: Mutex<HashMap<String, tauri::menu::MenuItem<R>>>,
}

/// Written out rather than derived, because `derive(Default)` on a generic
/// struct demands `R: Default` --- and `Wry` is not. The map's emptiness is what
/// is being defaulted; the runtime parameter only names the item type.
impl<R: Runtime> Default for MenuItems<R> {
    fn default() -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
        }
    }
}

impl<R: Runtime> MenuItems<R> {
    /// How many items are being tracked. Zero before [`install`] has run.
    pub fn len(&self) -> usize {
        self.items.lock().expect("menu items lock").len()
    }

    /// Whether nothing has been installed yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Builds the whole bar from `spec` and installs it.
///
/// # Errors
///
/// The main thread could not be reached, or AppKit refused an item --- which in
/// practice means an accelerator string the parser does not accept. A refusal is
/// returned rather than logged: a menu that is silently half-built looks exactly
/// like one whose commands are missing, and this is the code path a reader
/// notices only by the absence of what they were looking for.
pub async fn install<R: Runtime>(app: &AppHandle<R>, spec: Vec<SectionSpec>) -> Result<(), String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.blocking_send(build(&handle, &spec));
    })
    .map_err(|e| format!("could not reach the main thread to build the menu: {e}"))?;
    rx.recv()
        .await
        .ok_or_else(|| "the menu builder did not answer".to_string())?
}

/// Sets each named command's item enabled or disabled.
///
/// Ids the menu does not have are ignored rather than refused. The frontend
/// sends the enablement of every command *it* put in the layout, and a command
/// whose item failed to build is not one the reader can choose --- so an unknown
/// id here is a menu that is already missing something, which [`install`] has
/// already reported.
///
/// # Errors
///
/// The main thread could not be reached.
pub async fn set_enabled<R: Runtime>(
    app: &AppHandle<R>,
    state: HashMap<String, bool>,
) -> Result<(), String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let items = handle.state::<MenuItems<R>>();
        let map = items.items.lock().expect("menu items lock");
        let mut failed = Vec::new();
        for (id, want) in &state {
            if let Some(item) = map.get(id) {
                if let Err(e) = item.set_enabled(*want) {
                    failed.push(format!("{id}: {e}"));
                }
            }
        }
        let _ = tx.blocking_send(if failed.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "could not update {} menu item(s): {}",
                failed.len(),
                failed.join(", ")
            ))
        });
    })
    .map_err(|e| format!("could not reach the main thread to update the menu: {e}"))?;
    rx.recv()
        .await
        .ok_or_else(|| "the menu updater did not answer".to_string())?
}

/// Builds and sets the menu. Main thread only.
fn build<R: Runtime>(app: &AppHandle<R>, spec: &[SectionSpec]) -> Result<(), String> {
    let mut built = HashMap::new();
    let mut menu = MenuBuilder::new(app);

    for section in spec {
        let mut submenu = SubmenuBuilder::new(app, &section.title);
        // No predefined About here, and that is the correction rather than an
        // omission. This branch added `PredefinedMenuItem::about` until
        // 2026-08-21, and `app.about` -- our own, which answers the same
        // question and is the only answer on Windows -- leads the application
        // section of `menubar.ts`. So the menu carried "About tpdf" twice, one
        // opening the platform panel and one writing the version into the
        // header, which is exactly the "one term per concept" failure this menu
        // exists to fix. Ours is the one kept: it says the same thing in both
        // surfaces the reader has, the palette and the bar, and on both
        // platforms. Nothing else predefined belongs at the top -- Quit and
        // Services are still added below, where the platform puts them.
        //
        // Nothing could see this. The platform's items are built here and our
        // titles arrive from the frontend, so neither side holds both lists,
        // and no test in either language compares a label against a label.
        // `scripts/menu_check.py` reads the real menu bar for that reason.
        for item in &section.items {
            match item {
                ItemSpec::Separator => submenu = submenu.separator(),
                ItemSpec::Command {
                    id,
                    title,
                    accelerator,
                    enabled,
                } => {
                    let mut entry = MenuItemBuilder::with_id(id, title).enabled(*enabled);
                    if let Some(accelerator) = accelerator {
                        entry = entry.accelerator(accelerator);
                    }
                    let entry = entry.build(app).map_err(|e| {
                        // The accelerator is named because it is the only part
                        // of an item that can be rejected, and the message the
                        // parser gives does not say which item it came from.
                        format!(
                            "could not build the menu item {title:?} (accelerator {accelerator:?}): {e}"
                        )
                    })?;
                    built.insert(id.clone(), entry.clone());
                    submenu = submenu.item(&entry);
                }
            }
        }
        if section.app {
            submenu = submenu
                .separator()
                .item(&PredefinedMenuItem::services(app, None).map_err(why)?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None).map_err(why)?)
                .item(&PredefinedMenuItem::hide_others(app, None).map_err(why)?)
                .item(&PredefinedMenuItem::show_all(app, None).map_err(why)?)
                .separator()
                .item(&PredefinedMenuItem::quit(app, None).map_err(why)?);
        }
        menu = menu.item(&submenu.build().map_err(why)?);
    }

    // Last, and predefined throughout: every item in it is the window manager's
    // rather than the application's, so there is nothing for the registry to
    // own.
    let window = SubmenuBuilder::new(app, "Window")
        .item(&PredefinedMenuItem::minimize(app, None).map_err(why)?)
        .item(&PredefinedMenuItem::maximize(app, None).map_err(why)?)
        .separator()
        .item(&PredefinedMenuItem::fullscreen(app, None).map_err(why)?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None).map_err(why)?)
        .build()
        .map_err(why)?;
    menu = menu.item(&window);

    let menu = menu.build().map_err(why)?;
    app.set_menu(menu).map_err(why)?;

    let items = app.state::<MenuItems<R>>();
    *items.items.lock().expect("menu items lock") = built;
    Ok(())
}

fn why(e: tauri::Error) -> String {
    format!("could not build the menu: {e}")
}

/// Forwards a chosen item to the frontend, which runs it through the registry.
///
/// The id travels rather than the action, so that a menu click and a palette
/// entry reach the *same* `run` --- including its enabled guard and its argument
/// handling. A menu that called into Rust directly would be a second
/// implementation of every command it lists.
pub fn forward<R: Runtime>(app: &AppHandle<R>, id: &str) {
    // A send that fails is a window that has gone; there is nothing to do about
    // it and nothing worth saying.
    let _ = app.emit(RUN_EVENT, id.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's tag names are part of the wire contract with `menubar.ts`,
    /// and nothing else checks them: a rename on either side produces a
    /// deserialisation failure at runtime, inside a command whose only visible
    /// effect is that no menu appears.
    #[test]
    fn a_separator_and_a_command_are_told_apart_by_their_tag() {
        let spec: Vec<SectionSpec> = serde_json::from_str(
            r#"[{"title":"File","app":false,"items":[
                 {"kind":"separator"},
                 {"kind":"command","id":"file.open","title":"Open document",
                  "accelerator":"CmdOrCtrl+O","enabled":true}]}]"#,
        )
        .expect("the wire shape parses");
        assert_eq!(spec.len(), 1);
        assert!(matches!(spec[0].items[0], ItemSpec::Separator));
        match &spec[0].items[1] {
            ItemSpec::Command {
                id,
                accelerator,
                enabled,
                ..
            } => {
                assert_eq!(id, "file.open");
                assert_eq!(accelerator.as_deref(), Some("CmdOrCtrl+O"));
                assert!(enabled);
            }
            ItemSpec::Separator => panic!("the second item is a command"),
        }
    }

    /// A missing accelerator is the ordinary case for the five chords a text
    /// field claims, so `null` has to survive the wire as `None` rather than as
    /// a parse failure that would take the whole menu with it.
    #[test]
    fn an_absent_accelerator_parses_as_none() {
        let spec: Vec<SectionSpec> = serde_json::from_str(
            r#"[{"title":"Edit","app":true,"items":[
                 {"kind":"command","id":"edit.undo","title":"Undo",
                  "accelerator":null,"enabled":false}]}]"#,
        )
        .expect("the wire shape parses");
        assert!(spec[0].app);
        match &spec[0].items[0] {
            ItemSpec::Command {
                accelerator,
                enabled,
                ..
            } => {
                assert_eq!(accelerator.as_deref(), None);
                assert!(!enabled, "an item can arrive disabled");
            }
            ItemSpec::Separator => panic!("expected a command"),
        }
    }
}
