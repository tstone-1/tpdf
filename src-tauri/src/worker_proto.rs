//! The vocabulary the parent and its worker speak, and the framing it travels in.
//!
//! Split out of `worker.rs` along with the mappings, the handover and the argv
//! quoting, when that file had grown to 2,861 lines and four concerns. Nothing
//! changed in the move: `worker.rs` re-exports every item here, so
//! `crate::worker::Request` still names this `Request`.
//!
//! One JSON object per line, in each direction. Spike 0.5 measured what that
//! costs, and the number is why isolation is affordable at all:
//!
//! - **A control round trip costs 6 µs** and a 4 MB tile costs **0.11 ms**
//!   through shared memory, against 3.0 ms to hand the same tile to the webview.
//!   The boundary is about 1/27th of the UI boundary; isolation is not where the
//!   time goes.
//!
//! Two properties hold across everything below. A request names nothing the
//! worker could act on --- see [`Request`]. And a reply is the one thing crossing
//! back from where the hostile input is, so it is read under [`MAX_REPLY_BYTES`]
//! by `read_reply_line`, which bounds the read rather than the allocation.

use std::io::{BufRead, Read};

use serde::{Deserialize, Serialize};

/// A request from the parent, one JSON object per line on the worker's stdin.
///
/// Deliberately carries no path, no descriptor and no pointer: everything the
/// worker may touch was handed to it before it dropped its authority.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    /// Parse the mapped document and report its geometry.
    Open {
        /// Collect only page 1's size instead of the whole table. Enumerating
        /// 775 pages costs 86 ms and buys a scrollbar exactness the scroller
        /// estimates anyway (PLAN §4).
        lazy_geometry: bool,
    },
    /// Render one tile into the shared mapping.
    Tile {
        /// Identity this request may be withdrawn by. Zero is not withdrawable.
        rid: u64,
        page: u32,
        scale: f32,
        turns: u8,
        invert: bool,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        /// Whether to PNG-encode in the worker rather than send raw pixels.
        png: bool,
        /// The page's crop box as the reader's edits have it, or the file's own.
        ///
        /// Defaulted for the reason [`Request::Text`]'s is.
        #[serde(default)]
        crop: Option<[f32; 4]>,
    },
    /// Abandon a tile request, whether or not it has started.
    ///
    /// Handled on the worker's reader thread rather than in the request queue,
    /// because the point of it is to reach a render that is *already running* ---
    /// a queued withdrawal would arrive after the thing it withdraws.
    Withdraw { rid: u64 },
    /// Extract one page's characters and their positions.
    Text {
        page: u32,
        /// The page's crop box as the reader's edits have it, or the file's own.
        ///
        /// Defaulted so that a request written before crops existed still parses
        /// as the uncropped extraction it meant.
        #[serde(default)]
        crop: Option<[f32; 4]>,
    },
    /// Find a query's occurrences on one page.
    Search {
        page: u32,
        query: String,
        /// How to match. Defaulted so that a request written before the options
        /// existed still parses as the unrestricted search it meant.
        #[serde(default)]
        options: crate::search::Options,
        /// The previous page's last characters, so a phrase that runs over the
        /// break is found. Defaulted for the same reason `options` is.
        #[serde(default)]
        carry: Option<crate::search::Carry>,
    },
    /// The box one page's ink occupies, in the page's own space.
    ///
    /// Measured rather than read: see `crate::content`, which renders the page
    /// small and finds the bounding box of everything that is not paper. Per
    /// page and never document-wide --- it costs a render, and a reader crops
    /// the page in front of them.
    Content { page: u32 },
    /// One page's displayed size under a crop box, or under the file's own.
    ///
    /// The frontend lays out from this, and it cannot compute it: the crop is in
    /// the page's own space and the layout is in display space, and the turn
    /// between them is the page's `/Rotate`, which the frontend never sees.
    Geometry {
        page: u32,
        #[serde(default)]
        crop: Option<[f32; 4]>,
    },
    /// Read the document's outline.
    Outline,
    /// Read every comment in the document.
    ///
    /// Document-level and lazy, like [`Request::Mapping`] and for the same two
    /// reasons: it costs an `lopdf` parse, and nothing on the startup path wants
    /// it. A reader who never opens the comments panel never pays for it.
    Comments,
    /// Read every link in the document.
    ///
    /// Document-level, like [`Request::Comments`], and asked for once just after
    /// first paint rather than lazily: a reader clicking a cross-reference has
    /// not opened a panel first, so waiting for demand would mean the first
    /// click on any document does nothing.
    Links,
    /// Report, per page, whether the text means anything or PDFium is guessing.
    ///
    /// Document-level and lazy: it costs a full `lopdf` parse, so it is asked for
    /// when a reader is about to be told something false --- a search that found
    /// nothing --- rather than on every open. See `crate::encoding`.
    Mapping,
    /// Read what the document says about itself.
    ///
    /// Document-level and the laziest of the three `lopdf` requests: nothing
    /// asks for it until a reader opens the properties dialog.
    Properties,
    /// Build the update section for a save that only adds marks.
    ///
    /// **The one request that produces bytes for a file**, and it is here rather
    /// than in the coordinator because it is a *parse*: `save::append_update` is
    /// a pure function of the document's bytes and the plan, and the document's
    /// bytes are attacker-controlled. Doing it here puts it behind the same
    /// sandbox, deadline and restart as every other parse, in the process that
    /// has already parsed this document with `lopdf` for its comments, links and
    /// properties. `docs/THREAT-MODEL.md` residual risk 17 is what this narrows.
    ///
    /// It still names nothing the worker could act on, which is [`Request`]'s
    /// standing property: a `Plan` is page positions, marks and geometry, and
    /// its one field that is a fact about a *file* --- the fingerprint --- is
    /// `#[serde(skip)]` on the type, so it cannot travel in either direction.
    /// Every decision about the file on disk stays with the caller.
    Append {
        /// What to write. Never a path, never a destination.
        plan: crate::edits::Plan,
    },
    /// Try the document again with a reader's password.
    ///
    /// **The one request that carries a secret**, which is worth stating against
    /// this type's standing property rather than leaving to be noticed. It does
    /// not break it: a password names nothing the worker could act on, and it
    /// widens the worker's authority by nothing at all --- the bytes are already
    /// mapped into this process, and a key to bytes you are holding is not a new
    /// reach. What it buys is that they stop being noise.
    ///
    /// It travels on stdin, which is the private pipe the parent already writes
    /// every request down, and never in argv, which any process on the machine
    /// can read out of the process table.
    ///
    /// Answered by every worker, not only a locked one: a reader who typed a
    /// password for a document that did not need one gets it accepted rather
    /// than getting an error about the file. See `worker_child::unlock`.
    Unlock {
        /// The reader's password, verbatim. Not logged, and not carried past the
        /// load it is for.
        password: String,
    },
}

/// A reply, one JSON object per line on the worker's stdout.
///
/// Payloads travel through the shared mapping, never inline: measured at
/// 0.11 ms against 0.61 ms down the pipe for 4 MB, and the mapping is where
/// PDFium renders to directly, so the pixel path has no copy in it at all.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    /// Bytes written into the tile mapping, for a payload-bearing reply.
    #[serde(default)]
    pub bytes: usize,
    /// Set when a tile was withdrawn rather than rendered.
    ///
    /// Distinct from an error and from an empty tile: there is nothing to draw,
    /// and a caller that painted this as blank would erase content it had.
    #[serde(default)]
    pub abandoned: bool,
    /// Set when the document is encrypted and the password it was given --- which
    /// may have been none --- did not open it.
    ///
    /// Distinct from an error, on the same reasoning [`abandoned`](Self::abandoned)
    /// is: nothing is wrong with the file. A caller that painted this as a
    /// failure would tell a reader their document is damaged when it is merely
    /// locked, and send them looking for a better copy of a file that is fine.
    ///
    /// It says nothing about *whether a password was tried*, because the worker
    /// cannot tell: PDFium answers `FPDF_ERR_PASSWORD` for both. Whoever is
    /// holding the conversation knows, and words it.
    #[serde(default)]
    pub locked: bool,
    /// JSON for a structured reply --- geometry, text, matches, an outline.
    #[serde(default)]
    pub json: Option<serde_json::Value>,
    /// Time inside PDFium.
    #[serde(default)]
    pub render_us: u64,
    /// Time spent encoding, zero for raw pixels.
    #[serde(default)]
    pub encode_us: u64,
}

impl Response {
    /// A failure carrying a diagnosable message.
    #[must_use]
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: message.into(),
            ..Default::default()
        }
    }

    /// A refusal a reader can answer: the document is locked.
    #[must_use]
    pub fn locked(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: message.into(),
            locked: true,
            ..Default::default()
        }
    }

    /// A success carrying a structured payload.
    ///
    /// # Errors
    ///
    /// Serialisation failure, which is a bug rather than a document problem and
    /// is reported as such rather than being unwrapped.
    pub fn json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_value(value) {
            Ok(json) => Self {
                ok: true,
                json: Some(json),
                ..Default::default()
            },
            Err(e) => Self::err(format!("could not serialise a reply: {e}")),
        }
    }
}

/// The document handed to a Windows worker that was started without one.
///
/// The counterpart of the macOS `SCM_RIGHTS` handover, and it has to be a
/// different mechanism rather than a different encoding: a Windows handle is a
/// number in one process's table and means nothing in another, so there is no
/// value the parent could simply *name*. What crosses is a `DuplicateHandle`
/// into the running child, which is a write the parent performs on the child's
/// handle table --- allowed because the parent is the more privileged of the two,
/// and the direction low integrity does not block. `handle` is therefore already
/// the child's number by the time this message says it.
///
/// **A message of its own rather than a [`Request`] variant**, and the type is
/// the argument. A handover is legal exactly once, before there is a document;
/// `Request` is the vocabulary of a worker that already has one. Folding it in
/// would make "adopt a second document" something the child has to *refuse* at
/// runtime, where keeping it out makes it something that cannot be said. It is
/// read off the same pipe requests later arrive on, at the one point in the
/// child's life where nothing else is reading that pipe.
#[cfg(windows)]
#[derive(Serialize, Deserialize)]
pub struct Handover {
    /// The document section, as a handle in the *child's* table.
    pub handle: usize,
    /// How much of it to map. A handle says nothing about length.
    pub len: usize,
}

/// The longest reply line the parent will read.
///
/// The worker is ours, but it is the process holding the attacker's document, so
/// its replies are the one thing crossing back from where the hostile input is.
/// `read_line` on a pipe is unbounded: a worker that has been made to emit an
/// endless line takes the *parent* down with it, which is precisely the failure
/// the boundary exists to prevent — the isolation would be perfect and the app
/// would still die.
///
/// Generous rather than tight, because a legitimate reply can be large: a dense
/// page's characters and boxes are hundreds of kilobytes of JSON, and a 10,000-
/// entry outline is a few megabytes. Tile pixels do not travel this way at all.
pub const MAX_REPLY_BYTES: u64 = 32 * 1024 * 1024;

/// Why a reply could not be read.
#[derive(Debug)]
pub(crate) enum ReplyError {
    /// The pipe reached end of file: the worker is gone.
    Closed,
    /// The line exceeded the limit, which is reported rather than truncated ---
    /// a truncated line would deserialise as a *malformed* reply and send the
    /// diagnosis to the protocol rather than to the worker that ran away.
    TooLong(u64),
    Io(std::io::Error),
}

/// Reads one newline-terminated reply, refusing one longer than `limit`.
///
/// Separate from [`crate::worker::Worker::read_reply`] so that it can be tested
/// at all: the thing worth asserting here is what happens on input a live worker
/// will not produce, and the only way to hand that over is to call this with a
/// reader that is not a pipe.
pub(crate) fn read_reply_line(reader: &mut impl BufRead, limit: u64) -> Result<String, ReplyError> {
    let mut line = String::new();
    // `take` bounds the read itself rather than checking the length afterwards,
    // which is the difference between refusing a huge line and allocating it
    // first and then complaining about it.
    match reader.take(limit).read_line(&mut line) {
        Ok(0) => Err(ReplyError::Closed),
        Ok(_) if line.ends_with('\n') => Ok(line),
        // No newline, and the two reasons for that are different diagnoses. At
        // the limit, the line was still going. Short of it, the pipe ended
        // mid-reply --- a worker that died while writing --- and calling that
        // "longer than 32 MB" would send the reader off to look at a size limit
        // when what happened is a crash the epitaph can name.
        Ok(read) if read as u64 >= limit => Err(ReplyError::TooLong(limit)),
        Ok(_) => Err(ReplyError::Closed),
        Err(e) => Err(ReplyError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{read_reply_line, ReplyError, Request, Response};

    #[test]
    fn a_request_survives_the_wire() {
        // The two sides are separate processes, so a field that fails to
        // round-trip fails at runtime and nowhere else.
        for request in [
            Request::Open {
                lazy_geometry: true,
            },
            Request::Tile {
                rid: 7,
                page: 3,
                scale: 1.5,
                turns: 2,
                invert: true,
                x: -4,
                y: 9,
                width: 1024,
                height: 768,
                png: false,
                // A crop that is not the page's own, so the four numbers are
                // carried rather than defaulted away by a serializer that
                // skips `None`.
                crop: Some([10.0, 20.0, 300.0, 400.0]),
            },
            Request::Withdraw { rid: 7 },
            Request::Text {
                page: 0,
                crop: None,
            },
            Request::Search {
                page: 1,
                query: "quartz".into(),
                options: crate::search::Options {
                    match_case: true,
                    whole_word: true,
                    regex: false,
                },
                carry: None,
            },
            Request::Outline,
            Request::Comments,
            Request::Links,
        ] {
            let line = serde_json::to_string(&request).expect("serialise");
            let back: Request = serde_json::from_str(&line).expect("deserialise");
            assert_eq!(
                format!("{request:?}"),
                format!("{back:?}"),
                "round trip changed {line}"
            );
        }
    }

    #[test]
    fn a_reply_distinguishes_abandoned_from_failed_and_from_empty() {
        // Three states a caller must tell apart: a tile that was withdrawn has
        // nothing to draw, and painting it as blank would erase what was there.
        let abandoned = Response {
            ok: true,
            abandoned: true,
            ..Default::default()
        };
        let empty = Response {
            ok: true,
            bytes: 0,
            ..Default::default()
        };
        let failed = Response::err("no such page");

        assert!(abandoned.ok && abandoned.abandoned);
        assert!(empty.ok && !empty.abandoned);
        assert!(!failed.ok && !failed.abandoned && !failed.error.is_empty());
    }

    #[test]
    fn an_ordinary_reply_is_read_whole() {
        // The control. Without it every assertion below is satisfied by a reader
        // that refuses everything, which is the shape a length bound fails in.
        let mut input = std::io::Cursor::new(b"{\"ok\":true}\n{\"ok\":false}\n".to_vec());
        let first = read_reply_line(&mut input, 64).expect("first line");
        assert_eq!(first, "{\"ok\":true}\n");
        // And the reader is left on the boundary, not somewhere inside the next
        // line: a bounded read that consumed too much would desynchronise the
        // stream and every later reply would be garbage.
        let second = read_reply_line(&mut input, 64).expect("second line");
        assert_eq!(second, "{\"ok\":false}\n");
    }

    #[test]
    fn a_reply_that_fills_the_limit_exactly_is_still_read() {
        // The boundary, from the permitted side. `take` counts the newline, so
        // an off-by-one here rejects the largest legitimate reply --- which
        // would only ever be discovered on a document big enough to produce one.
        let line = b"12345678\n";
        let mut input = std::io::Cursor::new(line.to_vec());
        assert_eq!(
            read_reply_line(&mut input, line.len() as u64).expect("exact fit"),
            "12345678\n"
        );
    }

    #[test]
    fn a_reply_longer_than_the_limit_is_refused_rather_than_truncated() {
        // A *complete* line that is merely too long, not a truncated one: with
        // no newline in it, an unbounded read would run out of input, return
        // without a newline, and be refused for that reason instead --- so the
        // first version of this test passed with the bound deleted.
        let mut line = vec![b'x'; 4096];
        line.push(b'\n');
        let mut input = std::io::Cursor::new(line);

        assert!(matches!(
            read_reply_line(&mut input, 64),
            Err(ReplyError::TooLong(64))
        ));
        // And the property the bound exists for, which no verdict can express:
        // that it stopped reading. The point of a limit is the memory never
        // allocated, so what has to be asserted is the input still waiting.
        assert_eq!(input.position(), 64, "the read was not bounded");
    }

    #[test]
    fn a_pipe_that_ends_mid_reply_is_a_dead_worker_and_not_an_oversized_one() {
        // The two ways a read ends without a newline, and they are different
        // diagnoses: "longer than 32 MB" sends the reader to look for a size
        // limit when what happened is a crash the epitaph can name.
        let mut input = std::io::Cursor::new(b"{\"ok\":tr".to_vec());
        assert!(matches!(
            read_reply_line(&mut input, 64),
            Err(ReplyError::Closed)
        ));
        // And an empty stream is the same answer, reached without reading
        // anything at all.
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(matches!(
            read_reply_line(&mut empty, 64),
            Err(ReplyError::Closed)
        ));
    }

    /// A password survives the wire, unchanged, on one line.
    ///
    /// The protocol is one JSON object per line in each direction, and the
    /// worker reads a password with `read_line`. So the two things that would
    /// break it are a variant that does not serialise and a value that could
    /// carry a newline through --- and a password is the one request field whose
    /// content is entirely a stranger's choice, quotes, backslashes and all.
    ///
    /// The awkward characters are the test rather than an aside: `serde_json`
    /// escapes them and this asserts it, because a password truncated at an
    /// embedded newline would be *refused*, which reads to a reader as their own
    /// typing being wrong.
    #[test]
    fn a_password_crosses_the_wire_on_one_line_and_arrives_unchanged() {
        for password in [
            "swordfish",
            "with \"quotes\" and \\backslashes\\",
            "with\na newline",
            "  leading and trailing  ",
            "\u{1F510} an astral one",
            "",
        ] {
            let line = serde_json::to_string(&Request::Unlock {
                password: password.to_string(),
            })
            .expect("a request serialises");
            assert!(!line.contains('\n'), "a request must be one line: {line:?}");

            let back: Request = serde_json::from_str(&line).expect("and parses back");
            let Request::Unlock { password: got } = back else {
                panic!("an unlock came back as something else: {line}");
            };
            assert_eq!(got, password, "the password did not survive {line}");
        }
    }

    /// `locked` is its own field, and defaults to false where it is absent.
    ///
    /// Two properties in one, and both are about the same mistake --- reading a
    /// refusal as more or less answerable than it is. A reply that says nothing
    /// about encryption is not a locked document, and a locked one is not an
    /// ordinary failure.
    #[test]
    fn a_reply_is_locked_only_when_it_says_so() {
        let plain: Response = serde_json::from_str(r#"{"ok":false,"error":"broken"}"#)
            .expect("a reply with no locked field parses");
        assert!(!plain.locked, "absent must mean not locked");

        let locked = Response::locked("This document is locked, and needs a password.");
        assert!(!locked.ok);
        assert!(locked.locked);
        let round: Response =
            serde_json::from_str(&serde_json::to_string(&locked).expect("serialises"))
                .expect("parses back");
        assert!(round.locked, "the flag did not survive the wire");
        assert_eq!(round.error, locked.error);

        // And the control, which is what stops "everything is locked" passing:
        // an ordinary failure built the ordinary way carries none of it.
        assert!(!Response::err("broken").locked);
    }
}
