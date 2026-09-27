// Where PDFKit draws a visible signature's ink: inside its rectangle, and nowhere else.
//
// Run: swift scripts/sign_visible_pdfkit.swift <original.pdf> <signed.pdf> <page> <left> <top> <right> <bottom>
//
// The rectangle is the one the signature was placed with, in points, in the page's DISPLAY
// space (after /Rotate, from the displayed top-left corner) --- the space the viewer hands
// back. Both files are rasterised at 2 px per point and compared pixel by pixel; a pixel
// counts as changed when any channel moved by more than 8/255. The original is the control:
// what it shows is the page without the signature, so every changed pixel is the appearance.
// A one-pixel band round the rectangle is reported apart, for antialiasing at its edge.
//
// Prints one line, `INSIDE <n> EDGE <n> OUTSIDE <n> OF <area>`, and exits 0; `sign-probe
// --visible` judges it. Called by that probe, not by a gate: it needs macOS.
import AppKit
import PDFKit

// sRGB bitmap of a thumbnail, as `signature_pdfkit_check.swift` does: `thumbnail` draws in
// the main display's colour space, and two renders compared in it would still agree, but a
// pixel read out of it is not the colour the file states.
func pixels(_ url: URL, page index: Int, scale: CGFloat) -> (NSBitmapImageRep, Int, Int) {
    guard let doc = PDFDocument(url: url), let page = doc.page(at: index) else {
        print("[FAIL] could not open \(url.path)")
        exit(2)
    }
    let box = page.bounds(for: .cropBox)
    let turned = page.rotation % 180 != 0
    let width = Int(((turned ? box.height : box.width) * scale).rounded())
    let height = Int(((turned ? box.width : box.height) * scale).rounded())
    let image = page.thumbnail(of: NSSize(width: width, height: height), for: .cropBox)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .calibratedRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        .retagging(with: .sRGB)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.white.setFill()
    NSRect(x: 0, y: 0, width: width, height: height).fill()
    image.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
    NSGraphicsContext.restoreGraphicsState()
    return (rep, width, height)
}

let args = CommandLine.arguments
guard args.count == 8, let index = Int(args[3]),
      let left = Double(args[4]), let top = Double(args[5]),
      let right = Double(args[6]), let bottom = Double(args[7]) else {
    print("usage: sign_visible_pdfkit.swift <original> <signed> <page> <left> <top> <right> <bottom>")
    exit(2)
}
let scale: CGFloat = 2
let (before, bw, bh) = pixels(URL(fileURLWithPath: args[1]), page: index, scale: scale)
let (after, aw, ah) = pixels(URL(fileURLWithPath: args[2]), page: index, scale: scale)
guard bw == aw && bh == ah else {
    print("[FAIL] the two render \(bw)x\(bh) and \(aw)x\(ah)")
    exit(2)
}
// Rows here run top-down, as NSBitmapImageRep's getPixel counts them.
let (x0, y0) = (Int((left * Double(scale)).rounded()), Int((top * Double(scale)).rounded()))
let (x1, y1) = (Int((right * Double(scale)).rounded()), Int((bottom * Double(scale)).rounded()))
var inside = 0, edge = 0, outside = 0
var a = [Int](repeating: 0, count: 4), b = [Int](repeating: 0, count: 4)
for y in 0..<bh {
    for x in 0..<bw {
        before.getPixel(&a, atX: x, y: y)
        after.getPixel(&b, atX: x, y: y)
        guard zip(a.prefix(3), b.prefix(3)).contains(where: { abs($0 - $1) > 8 }) else { continue }
        if x >= x0 && x < x1 && y >= y0 && y < y1 {
            inside += 1
        } else if x >= x0 - 1 && x <= x1 && y >= y0 - 1 && y <= y1 {
            edge += 1
        } else {
            outside += 1
        }
    }
}
print("INSIDE \(inside) EDGE \(edge) OUTSIDE \(outside) OF \((x1 - x0) * (y1 - y0))")
