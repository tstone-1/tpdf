// macOS producer fixture, using CoreText's public drawing API without rewriting
// any PDF operators. Run: swift testdata/make_textedit_quartz.swift <output.pdf>
// Compare the output with text-edit-probe --inspect before extending its grammar.
import Foundation
import CoreGraphics
import CoreText

guard CommandLine.arguments.count == 2 else {
    fputs("usage: make_textedit_quartz.swift <output.pdf>\n", stderr)
    exit(1)
}
let output = URL(fileURLWithPath: CommandLine.arguments[1])
var box = CGRect(x: 0, y: 0, width: 300, height: 240)
guard let context = CGContext(output as CFURL, mediaBox: &box, nil) else {
    fputs("[FAIL] cannot create PDF\n", stderr)
    exit(1)
}
context.beginPDFPage(nil)
let font = CTFontCreateWithName("Helvetica" as CFString, 12, nil)
for (text, y) in [("SYNTHETIC FIRST", 180.0), ("SYNTHETIC SECOND", 140.0)] {
    let string = NSAttributedString(string: text, attributes: [
        NSAttributedString.Key(kCTFontAttributeName as String): font
    ])
    context.textPosition = CGPoint(x: 40, y: y)
    CTLineDraw(CTLineCreateWithAttributedString(string), context)
}
context.endPDFPage()
context.closePDF()
print("[OK] wrote synthetic Quartz text fixture")
