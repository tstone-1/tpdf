//! One open document's extracted characters, kept so a second query is free.
//!
//! ## What it is for
//!
//! A whole-document search extracts every page. `text::extract` is a PDFium
//! call per page and the dominant cost of a scan, and a reader typing a
//! six-letter query into the find bar runs six scans --- so on the 775-page
//! corpus the same 775 extractions happen six times to answer six versions of
//! one question. Nothing between the keystrokes changes a single character.
//!
//! ## Why it holds rather than evicts
//!
//! **Fill and hold, and that is a decision rather than a simplification.** An
//! LRU is the obvious policy and is the worst possible one here: a scan touches
//! every page exactly once in the same order every time, so a cache smaller than
//! the document is emptied by each walk of precisely the entries the next walk
//! asks for first. `docs/TRAPS.md` records that shape --- *a single-entry cache
//! is evicted by the grid scan that was about to test it* --- and an LRU over a
//! sequential scan is the same defect with more bookkeeping. Holding what fits
//! and declining the rest degrades instead: a document larger than the budget
//! serves its first pages from memory and extracts the tail, every time, which
//! is strictly better than serving none of it.
//!
//! `progressive::PageCache` is an LRU and is right to be: it holds `FPDF_PAGE`
//! handles for a *reader*, who returns to the page in front of them.
//!
//! ## Why nothing invalidates it
//!
//! There is no invalidation hook, and the reason is a property of the process
//! rather than an omission. A worker holds one read-only mapping of one file for
//! its whole life; an edit that changes the bytes cannot reach it, because a
//! save that alters the document answers `save::Refusal::reopen`, and the reader
//! reopens --- which is a new document, in new workers, with new caches. The
//! reader's own rotation is applied when a tile is drawn and never here, and the
//! crop is part of the key rather than state. So an entry cannot go stale while
//! anything can still read it.
//!
//! That claim is worth stating rather than assuming, because it is the whole
//! safety argument: if a request ever mutated the document a worker holds, this
//! cache would serve the characters of the file as it was.

use std::collections::HashMap;
use std::sync::Arc;

/// A page under a crop, which is what an extraction is a function of.
///
/// The crop is carried as bits rather than as `f32`, because a key has to be
/// `Eq` and `Hash` and a float is neither --- and because bit equality is the
/// right test here anyway: two crops that differ in the last bit are two
/// different extractions, and `None` (the file's own box) is a third thing
/// again rather than a value.
pub type Key = (u32, Option<[u32; 4]>);

/// The key for a page under `crop`.
#[must_use]
pub fn key(page: u32, crop: Option<[f32; 4]>) -> Key {
    (page, crop.map(|c| c.map(f32::to_bits)))
}

/// How many characters may be held before the cache stops taking more.
///
/// Characters rather than entries, because pages differ by two orders of
/// magnitude: the budget an A0 vector sheet spends is not the budget a page of
/// a novel spends, and an entry count would either starve one or overspend on
/// the other.
///
/// **4,000,000, which is 16 MB of `u32` and about twice the 775-page corpus.**
/// Measured rather than picked: `text-heavy.pdf` extracts to 1,860,431
/// characters across its 775 pages, so the document a scan is slowest on fits
/// entirely, with room for a second document open beside it. Only the codes are
/// kept --- four bytes a character --- and not the boxes, which are sixteen more
/// and which search never reads.
pub const BUDGET_CHARS: usize = 4_000_000;

/// A document's extracted characters, page by page.
#[derive(Default)]
pub struct TextCache {
    entries: HashMap<Key, Arc<Vec<u32>>>,
    /// Characters held, so the budget is a running total rather than a walk.
    chars: usize,
}

impl TextCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is held for `key`, if anything.
    #[must_use]
    pub fn get(&self, key: Key) -> Option<Arc<Vec<u32>>> {
        self.entries.get(&key).map(Arc::clone)
    }

    /// Offers `codes` for `key`, and hands back what a caller should use.
    ///
    /// The returned `Arc` is the same one either way, so a caller never has to
    /// know whether the cache took it --- which is what stops "it did not fit"
    /// becoming a second code path at every call site.
    pub fn put(&mut self, key: Key, codes: Vec<u32>, budget: usize) -> Arc<Vec<u32>> {
        let held = Arc::new(codes);
        // An entry already there is left alone rather than replaced: it is the
        // same extraction of the same bytes, and replacing it would charge the
        // budget twice for one page.
        if self.entries.contains_key(&key) {
            return Arc::clone(self.entries.get(&key).expect("just checked"));
        }
        if self.chars + held.len() <= budget {
            self.chars += held.len();
            self.entries.insert(key, Arc::clone(&held));
        }
        held
    }

    /// How many pages are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many characters are held, against the budget.
    #[must_use]
    pub fn chars(&self) -> usize {
        self.chars
    }
}

#[cfg(test)]
mod tests {
    use super::{key, TextCache};

    /// A page put in comes back out, and it is the same allocation.
    #[test]
    fn a_page_put_in_comes_back() {
        let mut cache = TextCache::new();
        let held = cache.put(key(3, None), vec![65, 66, 67], 100);
        let found = cache.get(key(3, None)).expect("page 3 is held");
        assert_eq!(*found, vec![65, 66, 67]);
        assert!(
            std::sync::Arc::ptr_eq(&held, &found),
            "a hit has to hand back what was stored, not a copy --- copying a \
             page's characters on every hit would spend most of what the cache saves"
        );
        assert_eq!(cache.chars(), 3);
        assert_eq!(cache.len(), 1);
    }

    /// A page nobody stored is a miss, not an empty answer.
    #[test]
    fn a_page_never_stored_is_a_miss() {
        let cache = TextCache::new();
        assert!(cache.get(key(3, None)).is_none());
        assert!(cache.is_empty());
    }

    /// The same page under two crops is two entries.
    ///
    /// Character *boxes* move under a crop and character *indices* do not, so
    /// the two extractions carry the same codes today --- which is exactly why
    /// this has to be keyed rather than reasoned about. A caller that starts
    /// asking for cropped text would otherwise be served the uncropped page's
    /// answer with nothing anywhere disagreeing.
    #[test]
    fn a_crop_is_part_of_the_key() {
        let mut cache = TextCache::new();
        cache.put(key(1, None), vec![1], 100);
        assert!(
            cache.get(key(1, Some([0.0, 0.0, 10.0, 10.0]))).is_none(),
            "the file's own box and a reader's crop are different keys"
        );
        cache.put(key(1, Some([0.0, 0.0, 10.0, 10.0])), vec![2, 3], 100);
        assert_eq!(cache.len(), 2);
        assert_eq!(*cache.get(key(1, None)).expect("uncropped"), vec![1]);
    }

    /// Two crops differing only in the last bit are two keys.
    #[test]
    fn two_crops_a_bit_apart_are_two_keys() {
        let mut cache = TextCache::new();
        let near = f32::from_bits(10.0f32.to_bits() + 1);
        cache.put(key(1, Some([0.0, 0.0, 10.0, 10.0])), vec![1], 100);
        cache.put(key(1, Some([0.0, 0.0, near, 10.0])), vec![2], 100);
        assert_eq!(cache.len(), 2);
    }

    /// Past the budget the cache stops taking pages, and keeps the ones it has.
    ///
    /// The direction that matters, and the reason the policy is not an LRU: a
    /// walk over a document larger than the budget must not empty the cache of
    /// the pages the next walk asks for first. Evicting would leave the *last*
    /// pages of the scan held and every scan starts at the beginning, so the hit
    /// rate would be zero rather than partial.
    #[test]
    fn what_does_not_fit_is_declined_rather_than_evicting_what_does() {
        let mut cache = TextCache::new();
        cache.put(key(0, None), vec![0; 6], 10);
        let overflow = cache.put(key(1, None), vec![1; 6], 10);
        assert_eq!(*overflow, vec![1; 6], "the caller is served either way");
        assert!(
            cache.get(key(0, None)).is_some(),
            "the page that fitted has to survive the page that did not"
        );
        assert!(cache.get(key(1, None)).is_none());
        assert_eq!(cache.chars(), 6);

        // And a later page small enough to fit still gets in: the budget is a
        // ceiling on what is held, not a latch that closes on the first miss.
        cache.put(key(2, None), vec![2; 4], 10);
        assert!(cache.get(key(2, None)).is_some());
        assert_eq!(cache.chars(), 10);
    }

    /// Storing a page twice charges the budget once.
    #[test]
    fn a_page_stored_twice_is_charged_once() {
        let mut cache = TextCache::new();
        cache.put(key(0, None), vec![0; 5], 10);
        cache.put(key(0, None), vec![0; 5], 10);
        assert_eq!(cache.chars(), 5);
        assert_eq!(cache.len(), 1);
    }
}
