//! A sentence a reader may be shown, written so the window can translate it.
//!
//! `said!` is `format!` and returns the same English `String`. What it adds is
//! a name the frontend's tooling can find: `src/lib/said.test.ts` reads every
//! `said!` template out of this crate's sources and requires German for it in
//! `src/lib/said/`, and `src/lib/said.ts` recognises the English sentence in
//! the window, takes its arguments back out and renders the German template.
//! The command-line tool prints the English string as it always did.
//!
//! Three rules follow from how the window recognises a sentence, and the test
//! enforces each:
//!
//! - **One plain string literal**, no `concat!`, no raw string: the test reads
//!   the literal as text.
//! - **Every placeholder named**, `{path}` or `{path:?}`, never `{}`: German puts
//!   them in another order, and the name is how it says which.
//! - **No two templates a sentence could fit both.** A placeholder matches any
//!   text, so `could not read {what}` and `could not read {what}: {why}` are told
//!   apart by the longer literal, and two templates with the same literal text
//!   around different placeholders are not told apart at all.
//!
//! A sentence another sentence is put inside --- `the redaction did not run:
//! {e}` --- is translated with its argument, so the inner one should be a
//! `said!` too. Text from outside tpdf (an `io::Error`, a library's message, a
//! file name) stays as it is.

/// `format!` for a sentence the window may show. See the module note.
macro_rules! said {
    ($($arg:tt)*) => {
        ::std::format!($($arg)*)
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn said_is_format() {
        let path = "a.pdf";
        assert_eq!(said!("could not open {path}"), "could not open a.pdf");
        assert_eq!(said!("{n} of {m}", n = 1, m = 2), "1 of 2");
    }
}
