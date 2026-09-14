// Independent readback of text-edit-probe's synthetic or W3C output on macOS.
// swift scripts/text_edit_pdfkit.swift scratch/text-edit/worker [--latin1]
import Foundation
import PDFKit
import CoreGraphics

func fail(_ message: String) -> Never {
    print("[FAIL] \(message)")
    exit(1)
}
guard (2...4).contains(CommandLine.arguments.count) else { fail("expected probe output directory [--latin1|--browser|--browser-flow|--browser-latin1|--browser-overhang|--default-encoding] [--page=N]") }
let root = URL(fileURLWithPath: CommandLine.arguments[1])
var selected = 0
var variant = ""
var hasPage = false
for option in CommandLine.arguments.dropFirst(2) {
    if option.hasPrefix("--page=") {
        guard !hasPage, let index = Int(option.dropFirst(7)), index >= 0 else { fail("invalid or duplicate page index") }
        selected = index
        hasPage = true
    } else {
        guard variant.isEmpty, ["--latin1", "--browser", "--browser-flow", "--browser-latin1", "--browser-overhang", "--default-encoding", "--w3c-dummy"].contains(option) else { fail("unknown or conflicting option") }
        variant = option
    }
}
let latin1 = variant == "--latin1"
let overhang = variant == "--browser-overhang"
let cidLatin1 = variant == "--browser-latin1" || overhang
let browser = variant == "--browser" || cidLatin1
let browserFlow = variant == "--browser-flow"
let defaultEncoding = variant == "--default-encoding"
let w3c = variant == "--w3c-dummy"
let original = w3c ? "Dummy PDF file" : defaultEncoding ? "SYNTHETIC ' ` £ ß" : cidLatin1 ? "SYNTHETIC ÄÖÜ äöü ß" : latin1 ? "SYNTHETIC ÄÖÜ ß" : "SYNTHETIC FIRST"
let replacement = w3c ? "Dummy PDF fill" : defaultEncoding ? "£ ' ` ß" : overhang ? "ÖÄÜ äöü ß" : cidLatin1 ? "ÄÖÜ äöü ß" : latin1 ? "GEPRÜFT ß" : "EDITED FIRST"
guard let before = PDFDocument(url: root.appendingPathComponent("synthetic-before.pdf")),
      let after = PDFDocument(url: root.appendingPathComponent("synthetic-after.pdf")),
      before.pageCount == after.pageCount, before.pageCount <= 128, selected < before.pageCount
else { fail("invalid document or page count") }
let width = w3c ? 1192 : 600, height = w3c ? 1684 : 480
if w3c && (selected != 0 || before.pageCount != 1) { fail("expected one-page W3C fixture") }
// Fixed fixture regions, independent of the editor's reported run/hit box.
// Edge's first baseline is y=189.75 (row 100.5), second y=148.5 (row 183).
// The flow HTML has 40pt line height: its first baseline is 63.75pt from the top.
// Accented Verdana ink stays within the first 32..65pt band of the authored page.
// W3C's final fragment begins at x=166.8pt with baseline y=758.1pt on an A4 page.
let targetRows = w3c ? (136..<174) : cidLatin1 ? (64..<130) : browserFlow ? (108..<140) : browser ? (78..<110) : (85..<130)
let targetColumns = w3c ? (330..<368) : (76..<520)
if browserFlow && before.pageCount != 2 { fail("expected two browser flow pages") }
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
          page.string?.components(separatedBy: .whitespacesAndNewlines).filter({ !$0.isEmpty }).joined(separator: " ") == first + (w3c ? "" : " SYNTHETIC SECOND")
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
            if pageIndex == selected && targetColumns.contains(x) && targetRows.contains(y) { changedInside += 1 }
            else { changedOutside += 1 }
        }
    }
}
guard ink > 100, (pageIndex == selected ? changedInside > 50 : changedInside == 0), changedOutside == 0 else {
    fail("raster comparison: ink \(ink), inside \(changedInside), outside \(changedOutside)")
}
print("[PASS] PDFKit page \(pageIndex + 1): \(changedInside) changed pixels inside target, zero outside; text agrees")
}
