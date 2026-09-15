// Independent readback of text-edit-probe's synthetic or W3C output on macOS.
// swift scripts/text_edit_pdfkit.swift scratch/text-edit/worker [--latin1|--image]
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
        guard variant.isEmpty, ["--latin1", "--browser", "--browser-flow", "--browser-latin1", "--browser-overhang", "--default-encoding", "--w3c-dummy", "--agenda", "--passport", "--dash", "--cff-unicode", "--cff-ligatures", "--cid-ligatures", "--continued", "--inline", "--image"].contains(option) else { fail("unknown or conflicting option") }
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
let passport = variant == "--passport"
let agenda = variant == "--agenda"
let dash = variant == "--dash"
let original = ["--cff-ligatures", "--cid-ligatures"].contains(variant) ? "SYNTHETIC ffi ffi fi fl ff" : variant == "--cff-unicode" ? "SYNTHETIC \u{2212}\u{00a0}\u{2018}\u{2019}\u{2013}£" : dash ? "SYNTHETIC\u{2013}FIRST" : w3c ? "Dummy PDF file" : defaultEncoding ? "SYNTHETIC ' ` £ ß" : cidLatin1 ? "SYNTHETIC ÄÖÜ äöü ß" : latin1 ? "SYNTHETIC ÄÖÜ ß" : "SYNTHETIC FIRST"
let replacement = ["--cff-ligatures", "--cid-ligatures"].contains(variant) ? "EDITED ffi fi fl ff" : variant == "--cff-unicode" ? "EDITED £\u{2013}\u{2019}\u{2018}\u{00a0}\u{2212}" : dash ? "EDITED\u{2013}FIRST" : w3c ? "Dummy PDF fill" : defaultEncoding ? "£ ' ` ß" : overhang ? "ÖÄÜ äöü ß" : cidLatin1 ? "ÄÖÜ äöü ß" : latin1 ? "GEPRÜFT ß" : "EDITED FIRST"
guard let before = PDFDocument(url: root.appendingPathComponent("synthetic-before.pdf")),
      let after = PDFDocument(url: root.appendingPathComponent("synthetic-after.pdf")),
      before.pageCount == after.pageCount, before.pageCount <= 128, selected < before.pageCount
else { fail("invalid document or page count") }
let width = passport ? 828 : w3c || agenda ? 1192 : 600, height = passport ? 1112 : w3c || agenda ? 1684 : 480
if passport && (selected != 15 || before.pageCount != 16) { fail("expected passport page 16") }
if agenda && (selected > 1 || before.pageCount != 2) { fail("expected two-page agenda") }
if w3c && (selected != 0 || before.pageCount != 1) { fail("expected one-page W3C fixture") }
// Fixed fixture regions, independent of the editor's reported run/hit box.
// Edge's first baseline is y=189.75 (row 100.5), second y=148.5 (row 183).
// The flow HTML has 40pt line height: its first baseline is 63.75pt from the top.
// Accented Verdana ink stays within the first 32..65pt band of the authored page.
// W3C's final fragment begins at x=166.8pt with baseline y=758.1pt on an A4 page.
let targetRows = passport ? (926..<1056) : agenda ? (selected == 1 ? (80..<134) : (156..<200)) : w3c ? (136..<174) : cidLatin1 ? (64..<130) : browserFlow ? (108..<140) : browser ? (78..<110) : (85..<130)
let targetColumns = ["--continued", "--inline"].contains(variant) ? (76..<298) : passport ? (744..<776) : agenda ? (selected == 1 ? (220..<400) : (740..<880)) : w3c ? (330..<368) : (76..<520)
if browserFlow && before.pageCount != 2 { fail("expected two browser flow pages") }
func sameBounds(_ left: CGRect, _ right: CGRect) -> Bool {
    // lopdf writes Real coordinates at f32 precision. Compare that representation
    // rather than rejecting an unchanged box for decimal serialization rounding.
    [left.minX, left.minY, left.maxX, left.maxY].map(Float.init)
        == [right.minX, right.minY, right.maxX, right.maxY].map(Float.init)
}
if ["--continued", "--inline"].contains(variant) {
    let old = before.findString("SYNTHETIC SECOND", withOptions: [])
    let new = after.findString("SYNTHETIC SECOND", withOptions: [])
    guard old.count == 1, new.count == 1, let oldPage = before.page(at: 0), let newPage = after.page(at: 0) else { fail("missing continuation") }
    let a = old[0].bounds(for: oldPage), b = new[0].bounds(for: newPage)
    guard abs(a.minX-b.minX) < 0.0001, abs(a.minY-b.minY) < 0.0001,
          abs(a.width-b.width) < 0.0001, abs(a.height-b.height) < 0.0001 else { fail("following text moved") }
    print("[PASS] PDFKit: following text retains its position and bounds")
}
for pageIndex in 0..<before.pageCount {
var pictures = [[UInt8]]()
for (name, document) in [("before", before), ("after", after)] {
    let first = name == "after" && pageIndex == selected ? replacement : original
    guard let page = document.page(at: pageIndex) else { fail("missing page") }
    if variant == "--inline" {
        guard let source = before.page(at: pageIndex)?.string,
              source.components(separatedBy: original).count == 2 else { fail("missing original inline text") }
        let expected = name == "after" ? source.replacingOccurrences(of: original, with: replacement) : source
        guard page.string == expected else { fail("inline ActualText or surrounding text changed") }
    } else if agenda || passport {
        guard let sourceText = before.page(at: pageIndex)?.string else { fail("missing agenda text") }
        let oldText = passport ? "ILB 53 (09.22)" : selected == 0 ? "REGULAR" : "Community Hub"
        let newText = passport ? "ILB 53" : selected == 0 ? "ANNUAL" : "Community"
        if pageIndex == selected && sourceText.components(separatedBy: oldText).count != 2 { fail("wrong agenda source text") }
        let expected = name == "after" && pageIndex == selected ? sourceText.replacingOccurrences(of: oldText, with: newText) : sourceText
        guard page.string == expected else { fail("agenda text or adjacent content changed") }
    } else {
        guard page.string?.components(separatedBy: .whitespacesAndNewlines).filter({ !$0.isEmpty }).joined(separator: " ") == first.replacingOccurrences(of: "\u{00a0}", with: " ") + (w3c ? "" : " SYNTHETIC SECOND")
        else { fail("PDFKit text readback disagrees for \(name)") }
    }
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
    if variant == "--image" {
        // make_textedit_symbolic.py places the image at (40,40), 168.2x28.2pt.
        // Require visible image content as well as unchanged pixels on save.
        var imageInk = 0
        for y in 344..<400 {
            for x in 80..<416 {
                let offset = (y * width + x) * 4
                if (0..<3).contains(where: { pixels[offset + $0] < 200 }) { imageInk += 1 }
            }
        }
        guard imageInk > 100 else { fail("image region is blank for \(name)") }
        print("[PASS] \(name) image region contains \(imageInk) painted pixels")
    }
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
