// Independent pixel-position check for the generated +/-1 Tc fixtures.
// swift scripts/text_spacing_pdfkit.swift positive.pdf negative.pdf positive-after.pdf negative-after.pdf
import Foundation
import PDFKit
import CoreGraphics

func fail(_ message: String) -> Never {
    print("[FAIL] \(message)")
    exit(1)
}
guard CommandLine.arguments.count == 5 else {
    fail("expected positive/negative source and saved PDFs")
}

// Read painted columns: PDFKit's selection rectangles do not locate the
// glyph's actual left edge.
func glyphStarts(_ path: String) -> [Int] {
    guard let document = PDFDocument(url: URL(fileURLWithPath: path)),
          let page = document.page(at: 0) else { fail(path) }
    guard document.pageCount == 1,
          page.bounds(for: .mediaBox) == CGRect(x: 0, y: 0, width: 300, height: 240)
    else { fail("unexpected fixture geometry") }
    let width = 3000, height = 2400
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    pixels.withUnsafeMutableBytes { buffer in
        guard let context = CGContext(data: buffer.baseAddress, width: width, height: height,
            bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { fail("cannot create raster context") }
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.scaleBy(x: 10, y: 10)
        page.draw(with: .mediaBox, to: context)
    }
    var starts = [Int]()
    var wasInk = false
    // The first line's fixed 180pt baseline lies at bitmap row 600.
    // These geometric glyphs each occupy one connected interval of columns.
    for x in 0..<width {
        let ink = (425..<650).contains { y in pixels[(y * width + x) * 4] < 128 }
        if ink && !wasInk { starts.append(x) }
        wasInk = ink
    }
    return starts
}

for (positive, negative, text) in [
    (CommandLine.arguments[1], CommandLine.arguments[2], "SYNTHETIC FIRST"),
    (CommandLine.arguments[3], CommandLine.arguments[4], "EDITED FIRST")
] {
    let a = glyphStarts(positive), b = glyphStarts(negative)
    let indices = text.enumerated().filter { $0.element != " " }.map { $0.offset }
    guard a.count == indices.count && b.count == indices.count
    else { fail("unexpected glyph populations: \(a.count), \(b.count)") }
    for (n, index) in indices.enumerated() {
        let delta = a[n] - b[n]
        guard abs(delta - index * 20) <= 1
        else { fail("glyph \(n), character \(index): spacing delta \(delta)") }
    }
    print("[PASS] rendered glyph positions differ by 2pt per character across all \(indices.count) painted glyphs")
}
