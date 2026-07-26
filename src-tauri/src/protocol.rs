//! The `tile://` custom URI scheme.
//!
//! Tiles must never cross the IPC boundary as base64-in-JSON. A custom scheme
//! is served by the webview's own network stack, which avoids the ~33% base64
//! penalty and the main-thread JSON parse.
//!
//! It is *not* zero-copy, and this spike exists partly to measure what it
//! actually costs per request. See docs/PLAN.md sections 3 and 10.
//!
//! URL shape:
//!   tile://localhost/<doc>/<page>/<scale>/<x>/<y>/<w>/<h>?fmt=raw|png
//!
//! `scale` is device pixels per PDF point, expressed in thousandths so the path
//! stays integral (2000 = 2.0x).

use tauri::http::{Request, Response};
use tauri::UriSchemeResponder;

use crate::render::{RenderService, Tile, TileFormat, TileRequest};

/// Handles one `tile://` request. Never blocks the caller: the render service
/// invokes the responder from the render thread when the tile is ready.
pub fn handle(service: &RenderService, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let uri = request.uri().clone();

    let parsed = match parse(uri.path(), uri.query()) {
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
                Ok(tile) => ok(tile),
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

    let width = field("w", width)?;
    let height = field("h", height)?;
    if width <= 0 || height <= 0 || width > u16::MAX as i64 || height > u16::MAX as i64 {
        return Err(format!("tile size out of range: {width}x{height}"));
    }

    let format = match query.and_then(parse_format) {
        Some(f) => f,
        None => TileFormat::Raw,
    };

    Ok(TileRequest {
        doc: field("doc", doc)? as u32,
        page: field("page", page)? as u16,
        scale: scale_thousandths as f32 / 1000.0,
        x: field("x", x)? as i32,
        y: field("y", y)? as i32,
        width: width as u16,
        height: height as u16,
        format,
    })
}

fn parse_format(query: &str) -> Option<TileFormat> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key != "fmt" {
            return None;
        }
        match value {
            "raw" => Some(TileFormat::Raw),
            "png" => Some(TileFormat::Png),
            _ => None,
        }
    })
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
        .header("Access-Control-Expose-Headers", "X-Tile-Width, X-Tile-Height, X-Render-Us, X-Encode-Us")
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

fn bad_request(message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(400)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(message.as_bytes().to_vec())
        .expect("static header set is always valid")
}
