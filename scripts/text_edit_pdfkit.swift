// Independent readback of text-edit-probe's synthetic output on macOS.
// swift scripts/text_edit_pdfkit.swift scratch/text-edit/worker [--latin1]
import Foundation
import PDFKit
import CoreGraphics

func fail(_ message: String) -> Never {
    print("[FAIL] \(message)")
    exit(1)
}
guard (2...3).contains(CommandLine.arguments.count) else { fail("expected probe output directory [--latin1|--page=N|--browser]") }
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let option = CommandLine.arguments.count == 3 ? CommandLine.arguments[2] : ""
let latin1 = option == "--latin1"
let browser = option == "--browser"
let selected: Int
if option.hasPrefix("--page=") {
    guard let index = Int(option.dropFirst(7)), index >= 0 else { fail("invalid page index") }
    selected = index
} else {
    guard option.isEmpty || latin1 || browser else { fail("unknown option") }
    selected = 0
}
let original = latin1 ? "SYNTHETIC ÄÖÜ ß" : "SYNTHETIC FIRST"
let replacement = latin1 ? "GEPRÜFT ß" : "EDITED FIRST"
guard let before = PDFDocument(url: root.appendingPathComponent("synthetic-before.pdf")),
      let after = PDFDocument(url: root.appendingPathComponent("synthetic-after.pdf")),
      before.pageCount == after.pageCount, before.pageCount <= 128, selected < before.pageCount
else { fail("invalid document or page count") }
let width = 600, height = 480
// Fixed fixture regions, independent of the editor's reported run/hit box.
// Edge's first baseline is y=189.75 (row 100.5), second y=148.5 (row 183).
let targetRows = browser ? (78..<110) : (85..<130)
func sameBounds(_ left: CGRect, _ right: CGRect) -> Bool {
    // lopdf writes Real coordinates at f32 precision. Compare that representation
    // rather than rejecting an unchanged box for decimal serialization rounding.
    [left.minX, left.minY, left.maxX, left.maxY].map(Float.init)
        == [right.minX, right.minY, right.maxX, right.maxY].map(Float.init)
}
for pageIndex in 0..<before.pageCount {
var pictures = [[UInt8]]()
for (name, document) in [("before", before), ("after", after)] {
    let first = name == "after" && pageIndex == selected ? replacement : original
    guard let page = document.page(at: pageIndex),
          page.string?.components(separatedBy: .whitespacesAndNewlines).filter({ !$0.isEmpty }).joined(separator: " ") == first + " SYNTHETIC SECOND"
    else { fail("PDFKit text readback disagrees for \(name)") }
    guard let old = before.page(at: pageIndex),
          sameBounds(old.bounds(for: .mediaBox), page.bounds(for: .mediaBox)),
          sameBounds(old.bounds(for: .cropBox), page.bounds(for: .cropBox)), old.rotation == page.rotation
    else { fail("page geometry changed") }
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
            if pageIndex == selected && (76..<520).contains(x) && targetRows.contains(y) { changedInside += 1 }
            else { changedOutside += 1 }
        }
    }
}
guard ink > 100, (pageIndex == selected ? changedInside > 50 : changedInside == 0), changedOutside == 0 else {
    fail("raster comparison: ink \(ink), inside \(changedInside), outside \(changedOutside)")
}
print("[PASS] PDFKit page \(pageIndex + 1): \(changedInside) changed pixels inside target, zero outside; text agrees")
}
