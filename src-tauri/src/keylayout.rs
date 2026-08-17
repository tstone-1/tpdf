//! What the reader's keyboard prints on a key, by physical position.
//!
//! **Why this exists.** A shortcut is declared in `src/lib/keys.ts` as a
//! character, and one of them is declared as a *position* as well --- ⌘\, whose
//! character needs ⌥⇧7 on a German keyboard and therefore could not be typed
//! there at all. The position fixes the chord and creates a labelling problem:
//! the palette renders `⌘\` from the character while the key the reader must
//! actually press is the one printing `#`. A wrong label teaches a wrong
//! shortcut, which is the whole reason `keys.ts` was extracted in the first
//! place.
//!
//! The menu bar gets this right for free --- AppKit resolves an accelerator's
//! key against the active layout when it draws the item --- so without this the
//! application contradicts itself between its menu and its palette.
//!
//! **The web view cannot answer it.** `navigator.keyboard.getLayoutMap()` is the
//! browser API for exactly this question and WebKit does not implement it; it is
//! Chromium-only. So the answer has to come from the platform, which is what
//! this module is.
//!
//! **How.** `TISCopyCurrentKeyboardLayoutInputSource` gives the active layout,
//! `kTISPropertyUnicodeKeyLayoutData` its `UCKeyboardLayout` table, and
//! `UCKeyTranslate` maps a virtual key code through it. `kUCKeyActionDisplay` is
//! the action a menu or a key-cap legend wants --- what the key *shows*, with no
//! modifiers applied --- rather than what pressing it would insert.
//!
//! **The one table here is positions, not characters**, and that is the point:
//! `Backslash` is a `KeyboardEvent.code`, which is a physical position named
//! after what a US keyboard prints there, and the virtual key codes below are
//! that same position in the platform's own numbering. Neither side of the map
//! is a character, so neither goes stale when the layout changes.
//!
//! macOS only. Windows has no menu bar here and no second reader of the label to
//! disagree with, so the palette's own rendering stands there.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

use objc2_core_foundation::CFString;

/// Serialises every call into the Text Input Sources API.
///
/// **HIToolbox aborts the process** when TIS or TSM is entered from two threads
/// at once, and it says so in as many words: *"Text Input Sources or Text
/// Services Manager API is being called in two threads concurrently. If you are
/// a UI application, you must call TIS/TSM API on the main thread. If you are a
/// non-UI application ... you must not call TIS/TSM API from multiple threads
/// concurrently."* Not a crash to be diagnosed --- a deliberate refusal.
///
/// That is not a hypothetical here. `cargo test` runs tests in parallel, so the
/// two tests at the bottom of this file killed the whole binary on the first
/// run, and 470 unrelated tests died with it having reported nothing. A SIGABRT
/// is not a red test: there are no results at all to read.
///
/// This lock satisfies the non-UI half of the rule. The UI half is the caller's:
/// `lib.rs`'s command hops to the main thread, as `menu.rs` does, because inside
/// the application the first sentence applies rather than the second.
static LOCK: Mutex<()> = Mutex::new(());

/// A `KeyboardEvent.code` and the macOS virtual key code at the same position.
///
/// Only the punctuation positions, because those are the ones whose character
/// moves between layouts and therefore the only ones a binding names by
/// position. A letter or digit is asked for by character and rendered as itself.
///
/// The numbers are `kVK_ANSI_*` from `HIToolbox/Events.h`. They are positions on
/// the physical keyboard, which is why a US-flavoured name can sit beside a code
/// that prints `#` on the machine this was written on.
const POSITIONS: &[(&str, u16)] = &[
    ("Backquote", 50),
    ("Minus", 27),
    ("Equal", 24),
    ("BracketLeft", 33),
    ("BracketRight", 30),
    ("Backslash", 42),
    ("Semicolon", 41),
    ("Quote", 39),
    ("Comma", 43),
    ("Period", 47),
    ("Slash", 44),
];

/// `kUCKeyActionDisplay` --- what the key cap shows, not what pressing it types.
const ACTION_DISPLAY: u16 = 3;
/// `kUCKeyTranslateNoDeadKeysBit`, which is belt and braces here --- measured,
/// after a mutation flipping it survived.
///
/// The reasoning it was added on was that `Equal` is the acute-accent dead key
/// on a German layout, so translating with dead keys *enabled* would swallow the
/// call and return nothing. That is true of `kUCKeyActionDown` and false of the
/// action this asks for: setting the bit to 0 produces a byte-identical map, all
/// eleven positions included. Sensible in hindsight --- `kUCKeyActionDisplay`
/// asks what the key cap shows, and a key cap has no dead-key state.
///
/// Kept because it is the documented-correct argument for a lookup that wants a
/// legend rather than an insertion, and it costs nothing. Recorded rather than
/// quietly dropped, so the next person does not re-derive the wrong reason: the
/// mutation that flips it is a variant, not a gap, and there is no test to add.
const NO_DEAD_KEYS: u32 = 1;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *mut c_void;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const u8,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
    fn CFDataGetBytePtr(data: *mut c_void) -> *const u8;
    fn CFRelease(cf: *mut c_void);
}

/// What the active layout prints on each position in [`POSITIONS`].
///
/// Keyed by `KeyboardEvent.code`, so the answer drops straight into the table
/// the frontend already indexes by that. A position the layout has no glyph for
/// is **absent** rather than empty: the caller falls back to the character the
/// binding declares, and an empty string would render as a chord with no key.
///
/// Returns an empty map rather than failing when the layout cannot be read ---
/// some input sources (a handwriting or transliteration source) genuinely carry
/// no `UCKeyboardLayout`, and the fallback is the label the palette showed
/// before this existed. That is a degradation, not an error.
#[must_use]
pub fn positions() -> HashMap<String, String> {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = HashMap::new();
    // SAFETY: `TISCopyCurrentKeyboardLayoutInputSource` follows the Create rule,
    // so the source is owned here and released below on every path. The property
    // it yields follows the Get rule and must not be released.
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return out;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        if data.is_null() {
            CFRelease(source);
            return out;
        }
        let layout = CFDataGetBytePtr(data);
        if layout.is_null() {
            CFRelease(source);
            return out;
        }
        let kind = u32::from(LMGetKbdType());
        for &(code, key) in POSITIONS {
            if let Some(glyph) = translate(layout, key, kind) {
                out.insert(code.to_string(), glyph);
            }
        }
        CFRelease(source);
    }
    out
}

/// One position's glyph, or `None` when the layout has nothing to show for it.
///
/// # Safety
///
/// `layout` must point at a live `UCKeyboardLayout`.
unsafe fn translate(layout: *const u8, key: u16, kind: u32) -> Option<String> {
    let mut dead: u32 = 0;
    let mut len: usize = 0;
    let mut buf = [0u16; 8];
    // SAFETY: `buf` and the two out-parameters outlive the call, and the length
    // handed over is the buffer's own.
    let status = unsafe {
        UCKeyTranslate(
            layout,
            key,
            ACTION_DISPLAY,
            0,
            kind,
            NO_DEAD_KEYS,
            &raw mut dead,
            buf.len(),
            &raw mut len,
            buf.as_mut_ptr(),
        )
    };
    if status != 0 || len == 0 || len > buf.len() {
        return None;
    }
    let glyph = String::from_utf16_lossy(&buf[..len]);
    // A space, or anything else with no ink, would render as a chord whose key
    // is invisible --- worse than the character the binding declares, which is
    // at least a character.
    if glyph.trim().is_empty() {
        return None;
    }
    Some(glyph)
}

/// The active layout's identifier, for a diagnostic that has to name it.
///
/// Not used to decide anything --- the glyphs above are the answer, and a layout
/// *name* is exactly the kind of thing a caller would be tempted to branch on.
#[must_use]
pub fn source_id() -> Option<String> {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: as `positions`. The property follows the Get rule.
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let id = TISGetInputSourceProperty(source, kTISPropertyInputSourceID);
        let out = if id.is_null() {
            None
        } else {
            Some((*id.cast::<CFString>()).to_string())
        };
        CFRelease(source);
        out
    }
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyInputSourceID: *const c_void;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout is whatever the machine is set to, so this cannot assert a
    /// glyph --- it asserts the *shape*, which is what a caller depends on.
    ///
    /// The control is the count. An implementation that read nothing and an
    /// implementation that read everything both satisfy a per-entry loop, and
    /// only one of them is this module working.
    #[test]
    fn every_position_answers_with_a_single_visible_glyph() {
        let found = positions();
        // Every one, not most. `>= len - 1` was the first spelling and it is
        // too loose to be a check: turning off `NO_DEAD_KEYS` drops exactly one
        // position on this layout --- `Equal` is the acute-accent dead key, and
        // a dead key translated with dead keys enabled returns nothing --- so
        // the slack was precisely the size of a real defect. A layout that
        // genuinely lacks one of these should fail here and be read, not
        // absorbed.
        assert_eq!(
            found.len(),
            POSITIONS.len(),
            "read {} of {} positions: {found:?}",
            found.len(),
            POSITIONS.len()
        );
        for (code, glyph) in &found {
            assert!(
                POSITIONS.iter().any(|(name, _)| name == code),
                "{code} is not a position this module asks about"
            );
            assert!(!glyph.trim().is_empty(), "{code} answered blank");
            assert!(
                glyph.chars().count() <= 2,
                "{code} answered {glyph:?}, which is not a key cap"
            );
        }
    }

    /// Two positions must not answer with the same glyph, or the label this
    /// produces cannot tell a reader which key to press.
    ///
    /// It is also the strongest available check that the virtual key codes are
    /// right: a table where several entries named one key would collide here,
    /// and a table of plausible-but-wrong codes almost certainly would too.
    #[test]
    fn no_two_positions_print_the_same_glyph() {
        let found = positions();
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for (code, glyph) in &found {
            if let Some(other) = seen.insert(glyph.as_str(), code.as_str()) {
                panic!("{code} and {other} both print {glyph:?}");
            }
        }
    }

    /// The layout is readable at all. Separate from the glyph tests because a
    /// machine with no Unicode layout data would fail those for a reason that
    /// is not a defect, and this says which case you are in.
    #[test]
    fn the_active_layout_names_itself() {
        let id = source_id();
        assert!(
            id.is_some(),
            "no input source id; the layout was not readable"
        );
        assert!(!id.unwrap_or_default().is_empty());
    }
}
