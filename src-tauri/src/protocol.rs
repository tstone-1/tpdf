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

/// Handles one `tile://` request. Never blocks the caller: the render service
/// invokes the responder from the render thread when the tile is ready.
pub fn handle(service: &RenderService, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let uri = request.uri().clone();
    let path = uri.path();

    if let Some(rest) = path.trim_start_matches('/').strip_prefix("cancel/") {
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

    Ok(TileRequest {
        rid,
        doc: field("doc", doc)? as u32,
        page: page_index as u32,
        scale: scale_thousandths as f32 / 1000.0,
        x: field("x", x)? as i32,
        y: field("y", y)? as i32,
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
