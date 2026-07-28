//! The `tile://` custom URI scheme.
//!
//! Tiles must never cross the IPC boundary as base64-in-JSON. A custom scheme
//! is served by the webview's own network stack, which avoids the ~33% base64
//! penalty and the main-thread JSON parse.
//!
//! It is *not* zero-copy, and this spike exists partly to measure what it
//! actually costs per request. See docs/PLAN.md sections 3 and 10.
//!
//! URL shapes:
//!   tile://localhost/<doc>/<page>/<scale>/<x>/<y>/<w>/<h>?fmt=raw|png&rid=<n>
//!   tile://localhost/cancel/<rid>
//!
//! `scale` is device pixels per PDF point, expressed in thousandths so the path
//! stays integral (2000 = 2.0x).
//!
//! `rid` names the request so it can be withdrawn later; omitting it asks for a
//! tile that will be rendered whatever happens. The withdrawal is a second
//! request rather than an aborted first one, because a `fetch()` abort is not
//! carried across the custom-scheme boundary --- the responder is simply never
//! read from, while the render runs to completion regardless.

use tauri::http::{Request, Response};
use tauri::UriSchemeResponder;

use crate::render::{RenderService, Tile, TileFormat, TileOutcome, TileRequest};
use crate::worker::TILE_CAPACITY;

/// Handles one `tile://` request. Never blocks the caller: the render service
/// invokes the responder from the render thread when the tile is ready.
pub fn handle(service: &RenderService, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let uri = request.uri().clone();
    let path = uri.path();

    if let Some(rest) = withdrawal(path) {
        let response = match rest.parse::<u64>() {
            Ok(rid) => {
                service.cancel(rid);
                no_content()
            }
            Err(_) => bad_request(&format!("cancel needs a request id, got {rest:?}")),
        };
        responder.respond(response);
        return;
    }

    let parsed = match parse(path, uri.query()) {
        Ok(r) => r,
        Err(message) => {
            responder.respond(bad_request(&message));
            return;
        }
    };

    service.tile(
        parsed,
        Box::new(move |result| {
            let response = match result {
                Ok(TileOutcome::Rendered(tile)) => ok(tile),
                Ok(TileOutcome::Abandoned) => no_content(),
                Err(message) => bad_request(&message),
            };
            responder.respond(response);
        }),
    );
}

/// The request id a withdrawal names, if this path is one.
///
/// Matched on the whole first segment rather than as a string prefix: every
/// other path begins with a document number, but `cancel` is a word, and
/// `strip_prefix("cancel")` alone would also claim a hypothetical `/cancelled/…`
/// and hand the rest to an integer parse.
fn withdrawal(path: &str) -> Option<&str> {
    let (first, rest) = path.trim_start_matches('/').split_once('/')?;
    (first == "cancel").then_some(rest)
}

fn parse(path: &str, query: Option<&str>) -> Result<TileRequest, String> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    let [doc, page, scale, x, y, width, height] = segments.as_slice() else {
        return Err(format!(
            "expected 7 path segments (doc/page/scale/x/y/w/h), got {}",
            segments.len()
        ));
    };

    let field = |name: &str, raw: &str| -> Result<i64, String> {
        raw.parse::<i64>()
            .map_err(|_| format!("{name} is not an integer: {raw:?}"))
    };

    let scale_thousandths = field("scale", scale)?;
    if scale_thousandths <= 0 {
        return Err(format!("scale must be positive, got {scale_thousandths}"));
    }

    let page_index = field("page", page)?;
    if page_index < 0 || page_index > u32::MAX as i64 {
        return Err(format!("page out of range: {page_index}"));
    }

    let width = field("w", width)?;
    let height = field("h", height)?;
    if width <= 0 || height <= 0 || width > u16::MAX as i64 || height > u16::MAX as i64 {
        return Err(format!("tile size out of range: {width}x{height}"));
    }

    // Refused *here*, before anything renders it. The worker checks too
    // (`worker_child::render`), but only after `progressive::render_tile` has
    // allocated `width * height * 4` and drawn into it --- which at the wire
    // format's own maximum is a 17 GB allocation, made inside the process that is
    // holding the attacker's document. The boundary contains the damage; it does
    // not make paying for it sensible. Bounding the request is a subtraction.
    let bytes = width as usize * height as usize * 4;
    if bytes > TILE_CAPACITY {
        return Err(format!(
            "a {width}x{height} tile is {bytes} bytes and the shared mapping holds {TILE_CAPACITY}"
        ));
    }

    // Range-checked like every other field, rather than `as`-cast. A negative
    // document number silently became `u32::MAX` and an origin past `i32` wrapped
    // to a plausible one --- neither reaches anything dangerous today, and both
    // are the kind of quiet coercion this parser refuses everywhere else.
    let doc = field("doc", doc)?;
    if doc < 0 || doc > u32::MAX as i64 {
        return Err(format!("document out of range: {doc}"));
    }

    let x = field("x", x)?;
    let y = field("y", y)?;
    let in_device_range = |value: i64| (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value);
    if !in_device_range(x) || !in_device_range(y) {
        return Err(format!("tile origin out of range: {x},{y}"));
    }

    let format = match query.and_then(|q| param(q, "fmt")).map(parse_format) {
        Some(Some(f)) => f,
        Some(None) => return Err("fmt must be raw or png".into()),
        None => TileFormat::Raw,
    };

    // Zero is the "not withdrawable" sentinel, so a caller that wants
    // cancellation must number its requests from one. An unparseable rid is an
    // error rather than a silent fallback to zero: it would produce a tile that
    // looks ordinary and can never be cancelled.
    let rid = match query.and_then(|q| param(q, "rid")) {
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|_| format!("rid is not a request id: {raw:?}"))?,
        None => 0,
    };

    // Refused rather than reduced modulo four. A caller asking for five
    // quarter-turns has computed something wrong, and quietly rendering the one
    // turn it happens to mean would hide that in the one place --- a rotated
    // view --- where nobody would think to look for it.
    let turns = match query.and_then(|q| param(q, "turns")) {
        Some(raw) => match raw.parse::<u8>() {
            Ok(turns) if turns < 4 => turns,
            _ => return Err(format!("turns must be 0, 1, 2 or 3: {raw:?}")),
        },
        None => 0,
    };

    // Spelled out rather than "present means on", so that a caller which builds
    // the query wrongly --- `invert=0`, or `invert=false` from a stringified
    // boolean --- gets an error instead of a dark page it did not ask for. The
    // absent case is the only one that means light.
    let invert = match query.and_then(|q| param(q, "invert")) {
        Some("1") => true,
        None => false,
        Some(raw) => return Err(format!("invert must be 1 or absent: {raw:?}")),
    };

    Ok(TileRequest {
        rid,
        doc: doc as u32,
        page: page_index as u32,
        scale: scale_thousandths as f32 / 1000.0,
        turns,
        invert,
        x: x as i32,
        y: y as i32,
        width: width as u16,
        height: height as u16,
        format,
    })
}

/// First value of `key` in a query string, or `None` if it is absent.
fn param<'q>(query: &'q str, key: &str) -> Option<&'q str> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

fn parse_format(value: &str) -> Option<TileFormat> {
    match value {
        "raw" => Some(TileFormat::Raw),
        "png" => Some(TileFormat::Png),
        _ => None,
    }
}

fn ok(tile: Tile) -> Response<Vec<u8>> {
    let content_type = match tile.format {
        TileFormat::Raw => "application/octet-stream",
        TileFormat::Png => "image/png",
    };

    Response::builder()
        .status(200)
        .header("Content-Type", content_type)
        // The webview origin differs from the custom scheme's, so responses
        // must opt in explicitly or fetch() rejects them.
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Access-Control-Expose-Headers",
            "X-Tile-Width, X-Tile-Height, X-Render-Us, X-Encode-Us",
        )
        // Raw tiles carry no dimensions in-band; the frontend needs them to
        // build an ImageData without guessing.
        .header("X-Tile-Width", tile.width.to_string())
        .header("X-Tile-Height", tile.height.to_string())
        // Server-side timings, so the frontend can separate render cost from
        // transfer cost without a second measurement channel.
        .header("X-Render-Us", tile.render_us.to_string())
        .header("X-Encode-Us", tile.encode_us.to_string())
        .body(tile.bytes)
        .expect("static header set is always valid")
}

/// Served for a tile the caller withdrew, and for the withdrawal itself.
///
/// 204 rather than an error status: the request was understood and served, and
/// the deliberate absence of a body is exactly what 204 means. A 4xx would put a
/// red line in the console for the mechanism working as designed, and would be
/// indistinguishable from a malformed URL.
///
/// Acknowledging a withdrawal says nothing about whether it arrived in time ---
/// it may have raced the tile it was cancelling, and the caller learns that from
/// the tile's own response rather than from this one.
fn no_content() -> Response<Vec<u8>> {
    Response::builder()
        .status(204)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .expect("static header set is always valid")
}

fn bad_request(message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(400)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(message.as_bytes().to_vec())
        .expect("static header set is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field a different value, so a transposed pair fails rather than
    /// cancelling out. `w` and `h` differ from `x` and `y` for the same reason.
    const PATH: &str = "/3/17/1500/64/96/512/256";

    fn ok(path: &str, query: Option<&str>) -> TileRequest {
        parse(path, query).expect("expected this to parse")
    }

    #[test]
    fn every_path_segment_lands_in_its_own_field() {
        let req = ok(PATH, None);
        assert_eq!(req.doc, 3);
        assert_eq!(req.page, 17);
        assert_eq!(req.scale, 1.5);
        assert_eq!(req.x, 64);
        assert_eq!(req.y, 96);
        assert_eq!(req.width, 512);
        assert_eq!(req.height, 256);
    }

    #[test]
    fn a_tile_origin_may_be_negative() {
        // The scaled page is placed at a negative offset to window a tile out of
        // it, so this is a real request shape rather than a curiosity.
        let req = ok("/0/0/1000/-10/-20/64/64", None);
        assert_eq!((req.x, req.y), (-10, -20));
    }

    #[test]
    fn the_wrong_number_of_segments_is_refused() {
        assert!(parse("/1/2/1000/0/0/64", None).is_err());
        assert!(parse("/1/2/1000/0/0/64/64/9", None).is_err());
    }

    #[test]
    fn a_field_that_is_not_an_integer_is_refused() {
        assert!(parse("/1/two/1000/0/0/64/64", None).is_err());
    }

    #[test]
    fn a_scale_that_would_render_nothing_is_refused() {
        assert!(parse("/1/0/0/0/0/64/64", None).is_err());
        assert!(parse("/1/0/-1000/0/0/64/64", None).is_err());
    }

    #[test]
    fn a_tile_size_outside_the_wire_format_is_refused() {
        assert!(parse("/1/0/1000/0/0/0/64", None).is_err());
        assert!(parse("/1/0/1000/0/0/64/0", None).is_err());
        assert!(parse("/1/0/1000/0/0/65536/64", None).is_err());
        assert!(parse("/1/0/1000/0/0/64/65536", None).is_err());
    }

    #[test]
    fn a_tile_larger_than_the_shared_mapping_is_refused_before_it_is_rendered() {
        // 2048² RGBA is exactly the mapping, so this is the boundary from both
        // sides: one pixel row more does not fit and must be refused *here*,
        // rather than after a worker has allocated and drawn it.
        assert_eq!(ok("/0/0/1000/0/0/2048/2048", None).width, 2048);
        assert!(parse("/0/0/1000/0/0/2048/2049", None).is_err());
        assert!(parse("/0/0/1000/0/0/2049/2048", None).is_err());
        // And the wire format's own maximum, which is the allocation this bound
        // exists for: 65535² × 4 is about 17 GB.
        assert!(parse("/0/0/1000/0/0/65535/65535", None).is_err());
    }

    #[test]
    fn a_document_number_outside_the_wire_format_is_refused() {
        // `-1 as u32` is 4294967295, which names no document and so fails
        // harmlessly downstream --- which is exactly why the coercion survived.
        assert!(parse("/-1/0/1000/0/0/64/64", None).is_err());
        assert!(parse("/4294967296/0/1000/0/0/64/64", None).is_err());
        assert_eq!(ok("/4294967295/0/1000/0/0/64/64", None).doc, u32::MAX);
    }

    #[test]
    fn a_tile_origin_outside_the_wire_format_is_refused() {
        // A wrapped origin renders a real tile from the wrong part of the page,
        // which is the plausible-wrong-answer failure this parser refuses
        // everywhere else.
        assert!(parse("/1/0/1000/2147483648/0/64/64", None).is_err());
        assert!(parse("/1/0/1000/0/-2147483649/64/64", None).is_err());
        // The boundaries themselves stay valid, or the check is off by one in
        // the direction that silently refuses legitimate work.
        assert_eq!(
            ok("/1/0/1000/2147483647/-2147483648/64/64", None).x,
            i32::MAX
        );
        assert_eq!(
            ok("/1/0/1000/2147483647/-2147483648/64/64", None).y,
            i32::MIN
        );
    }

    #[test]
    fn a_page_outside_the_wire_format_is_refused() {
        assert!(parse("/1/-1/1000/0/0/64/64", None).is_err());
        assert!(parse("/1/4294967296/1000/0/0/64/64", None).is_err());
        // The boundary itself must still be accepted, or the check is off by one
        // in the direction that silently refuses valid work.
        assert_eq!(ok("/1/4294967295/1000/0/0/64/64", None).page, u32::MAX);
    }

    #[test]
    fn the_format_defaults_to_raw_and_is_otherwise_taken_from_the_query() {
        assert_eq!(ok(PATH, None).format, TileFormat::Raw);
        assert_eq!(ok(PATH, Some("fmt=raw")).format, TileFormat::Raw);
        assert_eq!(ok(PATH, Some("fmt=png")).format, TileFormat::Png);
    }

    #[test]
    fn an_unknown_format_is_refused_rather_than_defaulted() {
        // Defaulting would serve raw bytes to a caller that asked for a PNG and
        // will try to decode them as one.
        assert!(parse(PATH, Some("fmt=jpeg")).is_err());
    }

    #[test]
    fn an_absent_request_id_means_the_tile_cannot_be_withdrawn() {
        assert_eq!(ok(PATH, None).rid, 0);
        assert_eq!(ok(PATH, Some("fmt=raw")).rid, 0);
    }

    #[test]
    fn a_request_id_is_read_from_the_query_whichever_order_it_is_in() {
        assert_eq!(ok(PATH, Some("fmt=png&rid=42")).rid, 42);
        assert_eq!(ok(PATH, Some("rid=42&fmt=png")).rid, 42);
    }

    #[test]
    fn an_unreadable_request_id_is_refused_rather_than_defaulted() {
        // Defaulting to zero would produce a tile that looks ordinary and can
        // never be cancelled, which is the failure this whole path exists to
        // avoid and would be invisible in the response.
        assert!(parse(PATH, Some("rid=abc")).is_err());
        assert!(parse(PATH, Some("rid=-1")).is_err());
    }

    #[test]
    fn a_query_key_is_matched_whole() {
        // Both directions, because they fail to different mistakes: a key that
        // *extends* the sought one defeats a prefix match, and only a key that
        // precedes it defeats a suffix match. The first version of this test
        // checked only the second direction, so a `starts_with` mutation passed
        // it --- which is why every one of these was mutated rather than trusted.
        assert_eq!(ok(PATH, Some("fmtx=png")).format, TileFormat::Raw);
        assert_eq!(ok(PATH, Some("xfmt=png")).format, TileFormat::Raw);
        assert_eq!(ok(PATH, Some("ridx=42")).rid, 0);
        assert_eq!(ok(PATH, Some("grid=42")).rid, 0);
        assert_eq!(ok(PATH, Some("turnsx=2")).turns, 0);
        assert_eq!(ok(PATH, Some("returns=2")).turns, 0);
        assert!(!ok(PATH, Some("invertx=1")).invert);
        assert!(!ok(PATH, Some("noinvert=1")).invert);
    }

    #[test]
    fn a_page_is_not_inverted_unless_it_is_asked_for() {
        assert!(!ok(PATH, None).invert);
        assert!(!ok(PATH, Some("fmt=raw&rid=1&turns=2")).invert);
        assert!(ok(PATH, Some("invert=1")).invert);
        assert!(ok(PATH, Some("rid=7&turns=3&invert=1&fmt=png")).invert);
    }

    #[test]
    fn anything_other_than_one_is_refused_rather_than_read_as_light() {
        // `invert=0` and `invert=false` are what a caller stringifying a boolean
        // produces, and both would otherwise land on the default and render a
        // light page --- which is indistinguishable from the mode being off.
        assert!(parse(PATH, Some("invert=0")).is_err());
        assert!(parse(PATH, Some("invert=false")).is_err());
        assert!(parse(PATH, Some("invert=true")).is_err());
        assert!(parse(PATH, Some("invert=")).is_err());
    }

    #[test]
    fn an_absent_rotation_means_the_page_is_upright() {
        assert_eq!(ok(PATH, None).turns, 0);
        assert_eq!(ok(PATH, Some("fmt=raw&rid=1")).turns, 0);
    }

    #[test]
    fn every_quarter_turn_is_read_from_the_query() {
        for turns in 0..4u8 {
            assert_eq!(ok(PATH, Some(&format!("turns={turns}"))).turns, turns);
        }
        assert_eq!(ok(PATH, Some("rid=7&turns=3&fmt=png")).turns, 3);
    }

    #[test]
    fn a_rotation_outside_a_quarter_turn_is_refused_rather_than_wrapped() {
        // `turns=4` is the interesting one: it means "no rotation" under a
        // modulo and "the caller's arithmetic is wrong" otherwise, and the
        // render it would produce looks perfectly ordinary either way.
        assert!(parse(PATH, Some("turns=4")).is_err());
        assert!(parse(PATH, Some("turns=-1")).is_err());
        assert!(parse(PATH, Some("turns=cw")).is_err());
        assert!(parse(PATH, Some("turns=")).is_err());
    }

    #[test]
    fn a_withdrawal_is_recognised_by_its_whole_first_segment() {
        assert_eq!(withdrawal("/cancel/42"), Some("42"));
        assert_eq!(withdrawal("cancel/42"), Some("42"));
        assert_eq!(withdrawal("/cancelled/42"), None);
        assert_eq!(withdrawal("/3/17/1500/64/96/512/256"), None);
        assert_eq!(withdrawal("/cancel"), None);
    }
}
