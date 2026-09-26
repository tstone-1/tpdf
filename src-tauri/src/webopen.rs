//! The addresses behind a document's web links, kept where the webview is not.
//!
//! ## Why the frontend is given a number instead of a URL
//!
//! `docs/PLAN.md` §11 decided that tpdf opens web links and named T8 as the
//! part that needed work, because the obvious implementation sends the webview
//! a string a stranger wrote and then trusts it to come back unchanged. This
//! module is the alternative: the worker resolves each `/URI` into an address,
//! the app process takes the whole list here, and what continues to the
//! frontend is an **index** plus the two display halves.
//!
//! The property that buys is narrow and worth stating exactly. It is not that
//! the webview is untrusted --- `docs/THREAT-MODEL.md` residual risk 7 already
//! grants any script the webview runs the whole command surface, printing and
//! saving included. It is that **a URL is an outbound network request to a host
//! of the caller's choosing**, which is an exfiltration channel none of those
//! other commands is, and this arrangement bounds it to the addresses that were
//! already in a document the reader opened. An attacker who can put a URL in
//! the document could have done that anyway; one who can only run script in the
//! webview gains nothing.
//!
//! ## Two lists, because there are two scans
//!
//! Links and the outline are separate requests answered by separate replies, so
//! each numbers its web targets from zero and a token means nothing without
//! knowing which. [`Source`] is that, passed as its own argument rather than
//! packed into the token's high bits --- `AGENTS.md` has entries about one
//! number quietly meaning two things.
//!
//! ## What is *not* kept
//!
//! A grant. There is no "always allow this site", per `docs/PLAN.md` §11: an
//! allow-list established from a document a stranger sent turns one careless
//! click into a standing capability. Every open is confirmed, every time, and
//! this module holds nothing that outlives the document it came from.

use std::collections::HashMap;

use parking_lot::Mutex;

use crate::weburl::Web;

/// Which of a document's two scans a token was numbered by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A rectangle on a page --- `links.rs`.
    Links,
    /// A row in the sidebar --- `outline.rs`.
    Outline,
}

/// Every open document's web addresses, by source.
///
/// Tauri-managed state. One lock over the whole map rather than one per
/// document: it is taken twice per document open and once per click on a link,
/// so contention is not a quantity this needs to reason about.
#[derive(Default)]
pub struct Registry {
    by_document: Mutex<HashMap<(u32, Source), Addresses>>,
}

/// One scan's addresses, indexed by the token a target carries.
///
/// `Option` because a token is an **index**: an entry the boundary check could
/// not parse has to leave a hole rather than shorten the list, or every token
/// after it names somebody else's address. See [`Registry::adopt`].
type Addresses = Vec<Option<Web>>;

impl Registry {
    /// Takes a scan's addresses, leaving the list that reaches the frontend
    /// empty.
    ///
    /// **Draining rather than copying is what makes the guarantee checkable.**
    /// If this merely read the list, the reply would still carry every URL when
    /// it was serialized to the webview, and nothing would go red --- the
    /// failure would be invisible in every test that looks at behaviour.
    /// `urls` is empty afterwards, and `document_links` returns the same value
    /// it passed in, so the two facts are one fact.
    ///
    /// Each address is parsed **again** here. The worker already refused
    /// everything the allowlist refuses, and between then and now the value
    /// crossed a process boundary and a pipe, so this is the boundary check
    /// rather than a duplicate of the worker's: an entry that does not parse
    /// becomes a hole, which makes that one link unopenable rather than opened
    /// unchecked --- and leaves every other token pointing where it did.
    pub fn adopt(&self, document: u32, source: Source, urls: &mut Vec<String>) {
        // `map` and not `filter_map`: the token is an index, so an entry that
        // does not parse has to leave a **hole**. Dropping it would shorten the
        // list and silently re-point every target after it at somebody else's
        // address, which is the one failure here that opens the wrong page
        // rather than none.
        let parsed: Addresses = std::mem::take(urls)
            .iter()
            .map(|raw| Web::parse(raw))
            .collect();
        self.by_document.lock().insert((document, source), parsed);
    }

    /// The address a token names, or `None` when there is none.
    ///
    /// `None` covers every way a token can fail to name one --- a document that
    /// has been closed, a scan that has not arrived, an index past the end, and
    /// an entry [`adopt`](Self::adopt) could not parse. The caller reports them
    /// identically, because they are identical to a reader: nothing opens.
    pub fn address(&self, document: u32, source: Source, token: u32) -> Option<Web> {
        let held = self.by_document.lock();
        held.get(&(document, source))?.get(token as usize)?.clone()
    }

    /// Forgets a document's addresses and those of every file it imported from.
    ///
    /// Called when a document is closed. Not a memory bound --- the lists are
    /// small --- but the thing that makes a token from a closed document name
    /// nothing, which is the property [`address`](Self::address) is allowed to
    /// state.
    ///
    /// **`sources` is a parameter rather than a second call, because a second
    /// call is a call that can be left out --- and two of the three places that
    /// close a document had left it out.** An imported file is a document of the
    /// render service with a scan and a list of its own, and since 26.9.16 a
    /// reader follows a web link on one of its pages through *its* handle, so
    /// its list is reachable rather than merely held. A list that outlived the
    /// document that imported from it would sit under a handle the service is
    /// free to hand to another file, which is the hazard
    /// [`forget_all`](Self::forget_all) states. The signature is what makes
    /// forgetting it impossible to write; `imports::Held` is the same shape for
    /// the handle itself.
    ///
    /// Empty `sources` is the ordinary case and means exactly what the old
    /// single-document call meant.
    pub fn forget(&self, document: u32, sources: &[u32]) {
        let mut held = self.by_document.lock();
        for id in std::iter::once(&document).chain(sources) {
            held.remove(&(*id, Source::Links));
            held.remove(&(*id, Source::Outline));
        }
    }

    /// Forgets every document's addresses, for a webview that has just started.
    ///
    /// The counterpart of `edits::Edits::release_all`, and it exists for the
    /// same reason `release_documents` calls that one: a reloaded webview holds
    /// no document id, so everything here is unreachable. Document numbers are
    /// **not** reused --- both backends allocate `docs.len()` and leave a hole
    /// where a closed document was --- so what this prevents is lists held for
    /// the rest of the process under ids nothing will close again, not one
    /// document's addresses answering another's clicks.
    ///
    /// Returns how many scans were dropped, so the caller can say whether
    /// anything was being held rather than reporting a silent zero.
    pub fn forget_all(&self) -> usize {
        let mut held = self.by_document.lock();
        let count = held.len();
        held.clear();
        count
    }

    /// How many addresses are held for a document's scan.
    ///
    /// An accounting observable, and it exists because the alternative is a
    /// leak nothing can see: `forget` removing the wrong key, or `adopt` never
    /// being called, both leave a viewer working normally. `AGENTS.md` records
    /// that a leak no behaviour can observe needs an observable rather than a
    /// cleverer assertion.
    pub fn held(&self, document: u32, source: Source) -> usize {
        self.by_document
            .lock()
            .get(&(document, source))
            .map_or(0, Vec::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn adopting_empties_the_list_the_frontend_would_receive() {
        let registry = Registry::default();
        let mut list = urls(&["https://example.com/a", "https://example.com/b"]);
        registry.adopt(7, Source::Links, &mut list);
        assert!(
            list.is_empty(),
            "the addresses must not continue to the frontend: {list:?}"
        );
        assert_eq!(registry.held(7, Source::Links), 2);
    }

    #[test]
    fn a_token_names_the_address_at_its_index() {
        let registry = Registry::default();
        let mut list = urls(&["https://first.example/", "https://second.example/"]);
        registry.adopt(1, Source::Links, &mut list);
        assert_eq!(
            registry.address(1, Source::Links, 1).map(|w| w.host),
            Some("second.example".to_string())
        );
    }

    #[test]
    fn the_two_sources_do_not_share_a_numbering() {
        let registry = Registry::default();
        registry.adopt(
            1,
            Source::Links,
            &mut urls(&["https://from-links.example/"]),
        );
        registry.adopt(
            1,
            Source::Outline,
            &mut urls(&["https://from-outline.example/"]),
        );
        // Token 0 in both, and they must not be the same address --- which is
        // the case that would break if the two lists shared a key.
        assert_eq!(
            registry.address(1, Source::Links, 0).map(|w| w.host),
            Some("from-links.example".to_string())
        );
        assert_eq!(
            registry.address(1, Source::Outline, 0).map(|w| w.host),
            Some("from-outline.example".to_string())
        );
    }

    #[test]
    fn two_documents_do_not_share_a_numbering_either() {
        let registry = Registry::default();
        registry.adopt(1, Source::Links, &mut urls(&["https://one.example/"]));
        registry.adopt(2, Source::Links, &mut urls(&["https://two.example/"]));
        assert_eq!(
            registry.address(2, Source::Links, 0).map(|w| w.host),
            Some("two.example".to_string())
        );
    }

    #[test]
    fn a_token_past_the_end_names_nothing() {
        let registry = Registry::default();
        registry.adopt(1, Source::Links, &mut urls(&["https://example.com/"]));
        assert_eq!(registry.address(1, Source::Links, 1), None);
        assert_eq!(registry.address(1, Source::Links, u32::MAX), None);
    }

    #[test]
    fn a_document_that_was_never_scanned_names_nothing() {
        let registry = Registry::default();
        assert_eq!(registry.address(99, Source::Links, 0), None);
        assert_eq!(registry.held(99, Source::Links), 0);
    }

    #[test]
    fn forgetting_a_document_makes_its_tokens_name_nothing() {
        let registry = Registry::default();
        registry.adopt(3, Source::Links, &mut urls(&["https://example.com/"]));
        registry.adopt(3, Source::Outline, &mut urls(&["https://example.com/"]));
        registry.adopt(4, Source::Links, &mut urls(&["https://other.example/"]));

        registry.forget(3, &[]);

        assert_eq!(registry.address(3, Source::Links, 0), None);
        assert_eq!(registry.address(3, Source::Outline, 0), None);
        assert_eq!(registry.held(3, Source::Links), 0);
        assert_eq!(registry.held(3, Source::Outline), 0);
        // The control: forgetting one document must not forget another, which
        // is the mistake a single `retain` over the map would make.
        assert_eq!(
            registry.address(4, Source::Links, 0).map(|w| w.host),
            Some("other.example".to_string())
        );
    }

    /// A document takes the files it imported from with it.
    ///
    /// The case this exists for is a reader who inserts another file's pages,
    /// follows a link on one of them, and then saves: the imported file's
    /// handle goes back to the render service, and a list left under it would
    /// answer the next file's clicks with this one's addresses. Two of the
    /// three closing paths forgot exactly this until the second parameter made
    /// it unwritable.
    #[test]
    fn a_document_takes_the_files_it_imported_from_with_it() {
        let registry = Registry::default();
        registry.adopt(10, Source::Links, &mut urls(&["https://opened.example/"]));
        registry.adopt(11, Source::Links, &mut urls(&["https://first.example/"]));
        registry.adopt(12, Source::Links, &mut urls(&["https://second.example/"]));
        registry.adopt(13, Source::Links, &mut urls(&["https://other.example/"]));

        registry.forget(10, &[11, 12]);

        assert_eq!(registry.address(10, Source::Links, 0), None);
        assert_eq!(
            registry.address(11, Source::Links, 0),
            None,
            "the first imported file's addresses went with the document"
        );
        assert_eq!(
            registry.address(12, Source::Links, 0),
            None,
            "and so did the second's, so it is not the first alone"
        );
        // The control: a document that imported nothing from either is
        // untouched, which is what says this removed named keys rather than
        // clearing the map.
        assert_eq!(
            registry.address(13, Source::Links, 0).map(|w| w.host),
            Some("other.example".to_string())
        );
    }

    #[test]
    fn forgetting_everything_leaves_no_document_answering() {
        let registry = Registry::default();
        registry.adopt(1, Source::Links, &mut urls(&["https://one.example/"]));
        registry.adopt(1, Source::Outline, &mut urls(&["https://one.example/"]));
        registry.adopt(2, Source::Links, &mut urls(&["https://two.example/"]));

        assert_eq!(registry.forget_all(), 3, "three scans were being held");

        assert_eq!(registry.address(1, Source::Links, 0), None);
        assert_eq!(registry.address(1, Source::Outline, 0), None);
        assert_eq!(registry.address(2, Source::Links, 0), None);
        assert_eq!(registry.forget_all(), 0, "and nothing is held twice");
    }

    #[test]
    fn re_scanning_a_document_replaces_its_list_rather_than_appending() {
        let registry = Registry::default();
        registry.adopt(
            5,
            Source::Links,
            &mut urls(&["https://old.example/", "https://old2.example/"]),
        );
        registry.adopt(5, Source::Links, &mut urls(&["https://new.example/"]));
        assert_eq!(registry.held(5, Source::Links), 1);
        assert_eq!(
            registry.address(5, Source::Links, 0).map(|w| w.host),
            Some("new.example".to_string())
        );
        // And the old list's second token is gone rather than surviving under
        // an index the new scan does not use.
        assert_eq!(registry.address(5, Source::Links, 1), None);
    }

    #[test]
    fn an_address_that_does_not_parse_leaves_a_hole_and_does_not_shift_the_rest() {
        let registry = Registry::default();
        // Nothing the worker sends can look like this --- it has already been
        // through `Web::parse`. The case exists because the two are separated
        // by a pipe, and because a list that silently shortened would re-point
        // every later token at somebody else's address.
        let mut list = urls(&[
            "https://first.example/",
            "javascript:alert(1)",
            "https://third.example/",
        ]);
        registry.adopt(1, Source::Links, &mut list);

        assert_eq!(registry.held(1, Source::Links), 3, "the hole is kept");
        assert_eq!(
            registry.address(1, Source::Links, 1),
            None,
            "and opens nothing"
        );
        assert_eq!(
            registry.address(1, Source::Links, 2).map(|w| w.host),
            Some("third.example".to_string()),
            "the token after the hole still names its own address"
        );
    }
}
