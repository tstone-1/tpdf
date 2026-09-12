// Independent readback of text-edit-probe's synthetic output on macOS.
// swift scripts/text_edit_pdfkit.swift scratch/text-edit/worker [--latin1]
import Foundation
import PDFKit
import CoreGraphics

func fail(_ message: String) -> Never {
    print("[FAIL] \(message)")
    exit(1)
}
guard CommandLine.arguments.count == 2 || (CommandLine.arguments.count == 3 && CommandLine.arguments[2] == "--latin1") else { fail("expected probe output directory") }
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let latin1 = CommandLine.arguments.count == 3 && CommandLine.arguments[2] == "--latin1"
let original = latin1 ? "SYNTHETIC ÄÖÜ ß" : "SYNTHETIC FIRST"
let replacement = latin1 ? "GEPRÜFT ß" : "EDITED FIRST"
let width = 600, height = 480
var pictures = [[UInt8]]()
for (name, first) in [("synthetic-before.pdf", original), ("synthetic-after.pdf", replacement)] {
    guard let document = PDFDocument(url: root.appendingPathComponent(name)), document.pageCount == 1,
          let page = document.page(at: 0),
          page.string?.components(separatedBy: .whitespacesAndNewlines).filter({ !$0.isEmpty }).joined(separator: " ") == first + " SYNTHETIC SECOND"
    else { fail("PDFKit text readback disagrees for \(name)") }
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    pixels.withUnsafeMutableBytes { buffer in
        guard let context = CGContext(data: buffer.baseAddress, width: width, height: height,
            bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { fail("cannot create raster context") }
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.scaleBy(x: 2, y: 2)
        page.draw(with: .mediaBox, to: context)
    }
    pictures.append(pixels)
}
var changedInside = 0, changedOutside = 0, ink = 0
for y in 0..<height {
    for x in 0..<width {
        let offset = (y * width + x) * 4
        if pictures[0][offset] < 200 { ink += 1 }
        if (0..<3).contains(where: { pictures[0][offset + $0] != pictures[1][offset + $0] }) {
            // Bitmap row zero is at the top: PDF y=180 maps to row 120.
            if (76..<520).contains(x) && (85..<130).contains(y) { changedInside += 1 }
            else { changedOutside += 1 }
        }
    }
}
guard ink > 100, changedInside > 50, changedOutside == 0 else {
    fail("raster comparison: ink \(ink), inside \(changedInside), outside \(changedOutside)")
}
print("[PASS] PDFKit reads the replacement and preserved second block; \(changedInside) changed pixels inside, zero outside")
