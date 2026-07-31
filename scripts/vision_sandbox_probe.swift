// Can macOS Vision text recognition run inside tpdf's production worker sandbox?
//
//   swiftc -O -o /tmp/vision_probe scripts/vision_sandbox_probe.swift
//   /tmp/vision_probe                       # control: must read the string back
//   /tmp/vision_probe <profile.sb>          # applies that profile to itself first
//
// Extract the production profile from the source rather than pasting a copy here,
// so this cannot drift from what the worker actually applies:
//
//   python3 -c "import re,pathlib; s=pathlib.Path('src-tauri/src/worker.rs').read_text(); \
//     print(re.search(r'pub const SANDBOX_PROFILE: &str = \"\\\\\\n(.*?)\";', s, re.S) \
//     .group(1).replace('\\\\\"','\"'), end='')" > /tmp/prod.sb
//
// Measured 2026-07-31, macOS 26.5.2 -- see docs/TRAPS.md and src-tauri/src/ocr.rs.
//
// The whole shape of an OCR design turns on this. If Vision runs under
// `SANDBOX_PROFILE`, OCR can be another Request on the existing parser worker.
// If it does not, OCR must live outside that boundary -- which is defensible,
// since it consumes pixels we rendered rather than bytes an attacker wrote, but
// it is a different architecture and it should be chosen on evidence.
//
// The image is drawn in-process rather than read from disk, because the profile
// denies file reads and tpdf hands pixels to a worker through an inherited
// mapping, not a path. So this mirrors how the real thing would receive them.
//
// A positive control is the point: "Vision read nothing" and "Vision could not
// run" print the same way unless the probe insists on reading a string it put
// there itself.

import Foundation
import CoreGraphics
import CoreText
import Vision

let expected = "REDACTED INVOICE 4471"

func drawImage() -> CGImage? {
    let w = 900, h = 200
    guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
          let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                              bytesPerRow: 0, space: cs,
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))

    let font = CTFontCreateWithName("Helvetica" as CFString, 64, nil)
    // CoreText keys rather than AppKit's, so this links no UI framework.
    let attrs: [NSAttributedString.Key: Any] = [
        kCTFontAttributeName as NSAttributedString.Key: font,
        kCTForegroundColorAttributeName as NSAttributedString.Key:
            CGColor(red: 0, green: 0, blue: 0, alpha: 1),
    ]
    let line = CTLineCreateWithAttributedString(
        NSAttributedString(string: expected, attributes: attrs))
    ctx.textPosition = CGPoint(x: 30, y: 70)
    CTLineDraw(line, ctx)
    return ctx.makeImage()
}

setbuf(stdout, nil)

// Apply the profile to ourselves *after* launch, which is what the production
// worker does (`worker_child.rs` calls `sandbox_init` post-exec). Running this
// under `sandbox-exec` instead would apply it before exec and the process would
// die in dyld -- a different failure, and one that would read as "Vision cannot
// run sandboxed" when it only means the loader was denied its own reads.
typealias SandboxInit = @convention(c) (
    UnsafePointer<CChar>?, UInt64, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32

func applySandbox(_ profile: String) -> String? {
    guard let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "sandbox_init") else {
        return "sandbox_init not found in libSystem"
    }
    let fn = unsafeBitCast(sym, to: SandboxInit.self)
    var err: UnsafeMutablePointer<CChar>?
    let rc = profile.withCString { fn($0, 0, &err) }
    if rc != 0 {
        return err.map { String(cString: $0) } ?? "sandbox_init returned \(rc)"
    }
    return nil
}

if CommandLine.arguments.count > 1 {
    let path = CommandLine.arguments[1]
    guard let profile = try? String(contentsOfFile: path, encoding: .utf8) else {
        print("[FAIL] could not read the profile at \(path)")
        exit(2)
    }
    if let e = applySandbox(profile) {
        print("[FAIL] sandbox_init refused the profile: \(e)")
        exit(2)
    }
    print("[OK]   applied the profile to this process, post-launch")
}

guard let image = drawImage() else {
    print("[FAIL] could not draw the control image -- CoreGraphics/CoreText refused")
    exit(2)
}
print("[OK]   drew a \(image.width)x\(image.height) control image containing: \(expected)")

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false

let handler = VNImageRequestHandler(cgImage: image, options: [:])
do {
    try handler.perform([request])
} catch {
    print("[FAIL] Vision refused to run: \(error)")
    print("       This is the answer the design needs: OCR cannot live in the parser worker.")
    exit(3)
}

let observations = request.results ?? []
let read = observations.compactMap { $0.topCandidates(1).first?.string }
print("[INFO] Vision returned \(observations.count) observation(s)")
for r in read { print("       read: \(r)") }

let joined = read.joined(separator: " ")
if joined.contains("4471") && joined.uppercased().contains("REDACTED") {
    print("[OK]   Vision read the control string back -- it ran, and it can see")
    exit(0)
}
if observations.isEmpty {
    print("[FAIL] Vision ran but returned nothing at all.")
    print("       Note this is exactly what a *successful redaction* looks like, which")
    print("       is why the real verification gate needs this same control.")
    exit(4)
}
print("[FAIL] Vision returned text but not the control string")
exit(5)
