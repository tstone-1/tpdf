//! The one place that decides whether a string a stranger wrote is a web
//! address tpdf will open, and what a reader is shown before it does.
//!
//! ## Why this is a module and not four lines at the `/URI` call site
//!
//! `docs/PLAN.md` §11 decided on 2026-08-31 that tpdf opens web links, and the
//! whole of that decision is about the two strings this module produces. The
//! URL is written by whoever wrote the document, so every question here --- is
//! this scheme one we open, which characters may a reader be shown, what is
//! emphasised --- is a question about attacker-chosen bytes. Answering them in
//! one place with tests is the difference between a policy and a habit.
//!
//! Both readers of `/URI` reach it: [`crate::links`] parses the action
//! dictionary itself, [`crate::outline`] asks PDFium. `AGENTS.md` records what
//! two copies of one rule cost --- the two destination resolvers already needed
//! `links-probe --mode agree` to hold them together --- so the *policy* is not
//! duplicated even though the extraction is.
//!
//! ## The allowlist is on the parsed scheme, never on the string's prefix
//!
//! A denylist here would be the validation direction this repository already
//! has an entry about: a scheme nobody listed is a scheme that gets through, and
//! on Windows the interesting ones (`ms-msdt:`, `search-ms:`, and whatever a
//! locally installed application registered this morning) are exactly the ones
//! nobody has heard of. So `http` and `https` are named and everything else is
//! refused with the refusal it had before this feature existed.
//!
//! It is applied to [`Url::scheme`] rather than to the raw text because
//! `https:/\evil.example` and a URL with a leading control character are both
//! things a stranger can write, and both start with a prefix that a `starts_with`
//! check would accept. The parser is the thing that knows what the scheme is.
//!
//! ## The host is displayed as punycode, on purpose
//!
//! [`Web::host`] is the ASCII form --- `xn--80ak6aa92e.com`, never `аррӏе.com`.
//! Rendering the Unicode form is drawing the homoglyph attack on the attacker's
//! behalf: the two strings are visually identical and only one of them is where
//! the reader is going. The `url` crate stores hosts IDNA-encoded, so this is
//! the form it hands back; [`Web::parse`] asserts the result is ASCII rather
//! than trusting that, because the display guarantee is the whole point and a
//! guarantee inherited from a dependency's documentation is not one this file
//! can check.

use url::{Position, Url};

/// Longest raw `/URI` string this will look at, in bytes.
///
/// A bound on attacker-chosen input before it reaches a parser, which is the
/// habit `docs/THREAT-MODEL.md` §T6 keeps everywhere else. Generous against
/// real links --- the longest in the 2,608-link regulation measured in
/// `docs/PLAN.md` §11 is a few hundred bytes --- and small enough that a
/// document cannot make the scan's memory its own to schedule.
pub const MAX_URL_BYTES: usize = 4_096;

/// Most characters of [`Web::rest`] a reader is shown before it is cut.
///
/// Characters rather than bytes, and cut on a character boundary, because this
/// number is about what fits beside the host in a dialog and a byte count would
/// make that width depend on the script the path is written in.
///
/// The path is the part a stranger writes to look reassuring ---
/// `/secure/your-bank.example.com/login` is a path, not a host --- so it is the
/// part that is truncated and de-emphasised while the host is not.
pub const MAX_REST_CHARS: usize = 80;

/// Appended to [`Web::rest`] when it was cut, so the reader knows it was.
pub const ELLIPSIS: char = '\u{2026}';

/// A web address tpdf is willing to open, split into what the reader is shown.
///
/// Constructing one is the only way to get past the allowlist, so a value of
/// this type is the claim that the scheme was checked --- the same shape as
/// `Target::Refused` being a type rather than a string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Web {
    /// What the operating system's opener is given, serialized by `url`.
    ///
    /// Never shown to a reader and never sent to the frontend: it is the whole
    /// address, where the two display fields are deliberately partial.
    pub url: String,
    /// The authority, ASCII, with a port when the URL names a non-default one.
    ///
    /// This is the field a reader is meant to look at, so it is complete and
    /// never truncated: a host cut in the middle is a host that can be made to
    /// end in whatever the attacker likes.
    pub host: String,
    /// Path, query and fragment, cut to [`MAX_REST_CHARS`].
    ///
    /// Display only, and secondary in the dialog. Empty for a bare host.
    pub rest: String,
}

impl Web {
    /// Reads a `/URI` string, or refuses it.
    ///
    /// `None` means the link keeps the refusal it had before web links were
    /// followed at all, which is why no reason is returned: every caller renders
    /// the same words for every rejection here, and a reason that named the
    /// scheme would be a document-written string reaching the frontend by
    /// another door.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.len() > MAX_URL_BYTES {
            return None;
        }
        // Before the parser, not after: `Url::parse` strips tabs and newlines
        // from its input rather than refusing them, so a URL carrying them
        // would come back looking clean and the refusal below would never fire.
        // A control character in a link is not something a real document does.
        if raw.chars().any(is_unsafe_display) {
            return None;
        }

        let url = Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }

        // An embedded credential is refused outright rather than shown with a
        // flag beside it. `docs/PLAN.md` §11 left that open between the two, and
        // refusing is the direction that can be loosened later without breaking
        // a reader who had come to rely on it. `https://your-bank.example.com@evil.example`
        // is the attack, and it is the one case where the host a reader would
        // pick out of the raw text is not the host the request goes to.
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }

        let host = url.host_str()?;
        if host.is_empty() || !host.is_ascii() {
            return None;
        }
        let host = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };

        let serialized = url.as_str();
        // The serialized form is what the opener gets, so it is what has to be
        // free of control characters --- checking only the input would miss a
        // character the parser decoded out of a percent escape.
        if serialized.chars().any(is_unsafe_display) {
            return None;
        }

        Some(Self {
            url: serialized.to_string(),
            host,
            rest: cut(&url[Position::BeforePath..]),
        })
    }
}

/// Whether a character must not appear in a URL tpdf opens or displays.
///
/// Controls and the Unicode separators, which is the set that can make one line
/// of text render as another. Not a markup question --- `check_webview_sinks.py`
/// answers that one --- but a legibility question: a host followed by a
/// right-to-left override renders with its end at its start.
fn is_unsafe_display(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{200E}' | '\u{200F}')
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// Cuts a path to [`MAX_REST_CHARS`], marking it when it cut.
fn cut(rest: &str) -> String {
    let mut out: String = rest.chars().take(MAX_REST_CHARS).collect();
    if rest.chars().nth(MAX_REST_CHARS).is_some() {
        out.push(ELLIPSIS);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_https_link_is_opened_and_split() {
        let web = Web::parse("https://example.com/a/b?q=1#f").expect("http(s) is opened");
        assert_eq!(web.host, "example.com");
        assert_eq!(web.rest, "/a/b?q=1#f");
        assert_eq!(web.url, "https://example.com/a/b?q=1#f");
    }

    #[test]
    fn plain_http_is_opened_too() {
        let web = Web::parse("http://example.com").expect("http is on the allowlist");
        assert_eq!(web.host, "example.com");
        // `url` normalises a bare host to a single-slash path, and that is what
        // a reader sees rather than an empty second line.
        assert_eq!(web.rest, "/");
    }

    /// Every scheme but the two, and the first four are the ones that matter.
    ///
    /// **The list started with the seven below the divider and none of them
    /// tested the allowlist.** A mutation deleting the scheme check outright
    /// SURVIVED: `javascript:`, `mailto:`, `data:`, `ms-msdt:` and a made-up
    /// scheme carry no host, and `file:///etc/passwd`'s host is empty, so every
    /// one of them was being refused two checks later by `host.is_empty()`. The
    /// case passed, for a reason that had nothing to do with the rule it was
    /// named for --- `docs/TRAPS.md`: *a control refused by a different guard
    /// than the one it was written for*.
    ///
    /// The four above the divider are the fix: each parses, each has a real
    /// host, and each therefore reaches the scheme check and nothing else.
    #[test]
    fn every_other_scheme_is_refused() {
        for raw in [
            // Special schemes with an authority, so only the allowlist can
            // refuse them.
            "ftp://example.com/x",
            "ws://example.com/socket",
            "wss://example.com/socket",
            "file://server/share/x",
            // ---- and the ones a host check would also have caught ----
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>",
            "ms-msdt:/id",
            "search-ms:query=x",
            "tpdf-madeup:whatever",
            "mailto:someone@example.com",
        ] {
            assert_eq!(Web::parse(raw), None, "{raw} must not be opened");
        }
    }

    #[test]
    fn a_scheme_is_read_from_the_parse_and_not_from_the_prefix() {
        // Leading whitespace: the parser strips it and reads the real scheme,
        // where a `starts_with` on the raw string sees neither `http` nor
        // `javascript` and would have to guess which.
        assert_eq!(Web::parse("  javascript:alert(1)"), None);
        assert!(Web::parse("  https://example.com/").is_some());
    }

    /// `https:/\evil.example` is `https://evil.example/`, and the parse is why
    /// that is the safe answer rather than the alarming one.
    ///
    /// **Written first as a refusal, and the test was wrong.** The premise was
    /// that a backslash makes the string something other than an https URL. It
    /// does not: the URL Standard treats `\` as `/` in a special scheme's
    /// authority, so this *is* a request to `evil.example` and every browser
    /// sends it there. A reader is therefore shown `evil.example`, which is
    /// where they would actually go --- and that is the argument for parsing
    /// rather than pattern-matching, stated the right way round. Refusing here
    /// would be tpdf disagreeing with every other client about what the
    /// document says, which is a worse failure than the one it was guarding.
    #[test]
    fn a_backslash_authority_is_read_the_way_a_browser_reads_it() {
        let web = Web::parse("https:/\\evil.example").expect("this is a real https URL");
        assert_eq!(web.host, "evil.example");
        assert_eq!(web.url, "https://evil.example/");
    }

    #[test]
    fn a_unicode_host_is_shown_as_punycode() {
        let web = Web::parse("https://\u{43E}\u{43F}\u{430}.com/x").expect("an IDN is a web link");
        assert!(
            web.host.starts_with("xn--"),
            "the display form must be punycode, got {:?}",
            web.host
        );
        assert!(web.host.is_ascii(), "punycode is ASCII by construction");
    }

    #[test]
    fn an_embedded_credential_is_refused() {
        assert_eq!(Web::parse("https://bank.example.com@evil.example/"), None);
        assert_eq!(Web::parse("https://user:pw@evil.example/"), None);
    }

    #[test]
    fn a_control_character_is_refused_even_though_the_parser_strips_it() {
        // The parser's own tolerance is the point: without the check *before*
        // it, `Url::parse` removes the newline and hands back a clean URL, so
        // the string a reader was shown would differ from the string in the
        // file with nothing reporting it.
        assert!(
            Url::parse("https://exa\nmple.com/").is_ok(),
            "the premise of this test is that the parser tolerates it"
        );
        assert_eq!(Web::parse("https://exa\nmple.com/"), None);
    }

    #[test]
    fn a_bidi_override_in_the_path_is_refused() {
        assert_eq!(Web::parse("https://example.com/\u{202E}gnp.exe"), None);
    }

    #[test]
    fn a_port_is_part_of_the_host_a_reader_is_shown() {
        let web = Web::parse("https://example.com:8443/x").expect("a port is legal");
        assert_eq!(web.host, "example.com:8443");
        // The default port for the scheme is not shown, because `url` removes
        // it --- and a host that sometimes carries `:443` and sometimes not is
        // one a reader has to think about.
        let plain = Web::parse("https://example.com:443/x").expect("a default port is legal");
        assert_eq!(plain.host, "example.com");
    }

    #[test]
    fn an_over_long_url_is_refused_before_it_is_parsed() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        assert!(long.len() > MAX_URL_BYTES);
        assert_eq!(Web::parse(&long), None);
    }

    #[test]
    fn a_long_path_is_cut_and_says_so() {
        let path = "b".repeat(MAX_REST_CHARS * 2);
        let web = Web::parse(&format!("https://example.com/{path}")).expect("a long path is legal");
        assert_eq!(web.rest.chars().count(), MAX_REST_CHARS + 1);
        assert!(web.rest.ends_with(ELLIPSIS));
        // And the host it is shown beside is untouched, which is the property
        // the truncation exists to protect.
        assert_eq!(web.host, "example.com");
    }

    #[test]
    fn a_path_that_exactly_fits_is_not_marked() {
        let path = "c".repeat(MAX_REST_CHARS - 1);
        let web = Web::parse(&format!("https://example.com/{path}")).expect("legal");
        assert_eq!(web.rest.chars().count(), MAX_REST_CHARS);
        assert!(!web.rest.ends_with(ELLIPSIS));
    }

    #[test]
    fn a_cut_lands_on_a_character_boundary() {
        // A path of astral characters, so a byte-wise cut would split one and
        // panic rather than merely truncating oddly.
        let path = "\u{1F600}".repeat(MAX_REST_CHARS * 2);
        let web = Web::parse(&format!("https://example.com/{path}")).expect("legal");
        assert!(web.rest.ends_with(ELLIPSIS));
    }

    #[test]
    fn a_host_that_is_not_a_host_is_refused() {
        assert_eq!(Web::parse("not a url at all"), None);
        assert_eq!(Web::parse(""), None);
        assert_eq!(Web::parse("https://"), None);
    }

    /// A single-label host is opened, and a run of slashes does not change it.
    ///
    /// The second premise this file got wrong. `https:///path-only` reads as
    /// `https://path-only/`, because a special scheme's parser collapses the
    /// slashes, and `path-only` is then an ordinary single-label host of the
    /// `localhost` and `intranet` family. Unusual in a document and not
    /// malformed, so refusing it would refuse a link that works.
    #[test]
    fn a_single_label_host_is_a_host() {
        let web = Web::parse("https:///path-only").expect("a single-label host is legal");
        assert_eq!(web.host, "path-only");
        assert_eq!(web.rest, "/");
    }
}
