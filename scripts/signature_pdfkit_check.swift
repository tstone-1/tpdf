// Independent pixel readback of signature fixtures, including source controls.
// Run: swift scripts/signature_pdfkit_check.swift <fixture-directory>
import PDFKit
import AppKit

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
        let bitmap = NSBitmapImageRep(data: image.tiffRepresentation!)!
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
