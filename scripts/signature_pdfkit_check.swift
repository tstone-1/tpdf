// Independent pixel readback of signature fixtures, including source controls.
// Run: swift scripts/signature_pdfkit_check.swift <fixture-directory>
import PDFKit
import AppKit

// Pixels in sRGB, whatever screen the Mac has. `PDFPage.thumbnail` rasterises in the MAIN
// DISPLAY's colour space, so the bytes of its bitmap are that display's values: on a MacBook's
// built-in "Color LCD" profile, device-RGB red (1,0,0) read (0.918, 0.2, 0.137), which is sRGB
// red expressed in Display P3, and this check failed on PDFs that were byte-identical and right.
// Drawing the thumbnail into a bitmap tagged sRGB makes AppKit convert it back; the same page
// read (1,0,0) exactly (measured 2026-09-25). It keeps the thumbnail's rotation handling.
func srgbPixels(_ image: NSImage, width: Int, height: Int) -> NSBitmapImageRep {
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
    return rep
}

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
var checks = 0
for prefix in ["signature", "signature-append", "signature-source"] {
    for turn in 0..<4 {
        let name = "\(prefix)-\(turn)"
        guard let doc = PDFDocument(url: directory.appendingPathComponent("\(name).pdf")),
              let page = doc.page(at: 0) else { fatalError("Could not open \(name)") }
        // The fixture's cropped display is 380x460, with the sides exchanged on odd turns.
        let width = turn % 2 == 0 ? 380 : 460
        let height = turn % 2 == 0 ? 460 : 380
        let image = page.thumbnail(of: NSSize(width: width, height: height), for: .cropBox)
        let bitmap = srgbPixels(image, width: width, height: height)
        let png = bitmap.representation(using: .png, properties: [:])!
        try png.write(to: directory.appendingPathComponent("\(name)-pdfkit.png"))
        let raster = NSBitmapImageRep(data: png)!
        guard raster.bitsPerSample == 8 && raster.samplesPerPixel >= 3 else { fatalError("Unexpected PNG pixel format") }
        // Sample away from the quadrant boundaries: PDFKit interpolates the tiny 2x2 image.
        let samples: [(Int, Int, [Double])] = [(60,60,[1,0,0]), (180,60,[0,1,0]), (60,180,[0,0,1]), (180,180,[1,1,1])]
        for (x, y, expected) in samples {
            // Read the exported PNG's channels. NSColor adds a display-profile conversion.
            var pixel = [UInt](repeating: 0, count: raster.samplesPerPixel)
            raster.getPixel(&pixel, atX: x * raster.pixelsWide / width, y: y * raster.pixelsHigh / height)
            let actual = pixel.prefix(3).map { Double($0) / 255 }
            let wanted = prefix == "signature-source" ? [1.0,1.0,1.0] : expected
            guard zip(actual, wanted).allSatisfy({ abs($0 - $1) < 0.03 }) else {
                print("[FAIL] \(name) at \(x),\(y): \(actual), expected \(wanted)")
                exit(1)
            }
            checks += 1
        }
    }
}
print("[PASS] PDFKit: \(checks) pixel checks across all rotations, append/rewrite and unedited controls")
