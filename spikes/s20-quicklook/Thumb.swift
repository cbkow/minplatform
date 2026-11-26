// S20: the thumbnail extension. It decodes nothing: it paints a placard that says whether the system asked US, and
// whether the sandbox let us list the file's folder and read a neighbour (green = a neighbour was read, magenta = not).
import AppKit
import QuickLookThumbnailing
import os

func s20Neighbours(_ url: URL) -> (Bool, String) {
    let dir = url.deletingLastPathComponent()
    guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir.path) else { return (false, "folder not listed") }
    guard let other = names.first(where: { $0 != url.lastPathComponent && !$0.hasPrefix(".") }) else { return (false, "listed \(names.count), no neighbour") }
    guard let fh = FileHandle(forReadingAtPath: dir.appendingPathComponent(other).path) else { return (false, "listed \(names.count), neighbour refused") }
    let n = fh.readData(ofLength: 16).count
    return (n > 0, "listed \(names.count), neighbour read \(n) bytes")
}

// The unified log did not give the lines back (`log show` found none), so each call also leaves a line in the extension's
// own container: ~/Library/Containers/<extension id>/Data/tmp/s20.log
func s20Note(_ line: String) {
    let p = NSTemporaryDirectory() + "s20.log", d = (line + "\n").data(using: .utf8)!
    if let h = FileHandle(forWritingAtPath: p) { h.seekToEndOfFile(); h.write(d); h.closeFile() } else { FileManager.default.createFile(atPath: p, contents: d) }
}

@_silgen_name("s20_thumb") func s20_thumb(_ path: UnsafePointer<CChar>, _ max: Int32, _ w: UnsafeMutablePointer<Int32>, _ h: UnsafeMutablePointer<Int32>) -> UnsafeMutablePointer<UInt8>?
@_silgen_name("s20_free") func s20_free(_ p: UnsafeMutablePointer<UInt8>, _ w: Int32, _ h: Int32)

/// The file's first picture through the family's FFmpeg (the Rust library of this spike), or nil.
func s20Decode(_ url: URL, _ max: Int32) -> CGImage? {
    var w: Int32 = 0, h: Int32 = 0
    guard let p = url.path.withCString({ s20_thumb($0, max, &w, &h) }) else { return nil }
    defer { s20_free(p, w, h) }
    let data = Data(bytes: p, count: Int(w * h * 4)) as CFData
    return CGImage(width: Int(w), height: Int(h), bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(w * 4), space: CGColorSpace(name: CGColorSpace.sRGB)!,
                   bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: CGDataProvider(data: data)!, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
}

@objc(S20Thumb) final class S20Thumb: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest, _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        let url = request.fileURL, ext = url.pathExtension.lowercased()
        let own = (try? FileHandle(forReadingFrom: url))?.readData(ofLength: 16).count ?? -1
        let (ok, what) = s20Neighbours(url)
        Logger(subsystem: "app.qcview.spike.s20", category: "thumb").notice("S20 thumb .\(ext, privacy: .public) own=\(own) \(what, privacy: .public)")
        let t0 = Date(), px = Int32(max(request.maximumSize.width, request.maximumSize.height) * request.scale)
        let img = ProcessInfo.processInfo.environment["S20_PLACARD"] == nil ? s20Decode(url, px) : nil
        s20Note("thumb .\(ext) own=\(own) \(what); decode \(img == nil ? "none" : "\(img!.width)x\(img!.height)") in \(Int(Date().timeIntervalSince(t0) * 1000)) ms")
        if let img {
            let k = min(request.maximumSize.width / CGFloat(img.width), request.maximumSize.height / CGFloat(img.height)), fit = CGSize(width: CGFloat(img.width) * k, height: CGFloat(img.height) * k)
            handler(QLThumbnailReply(contextSize: fit, currentContextDrawing: { NSGraphicsContext.current?.cgContext.draw(img, in: CGRect(origin: .zero, size: fit)); return true }), nil); return
        }
        let size = request.maximumSize
        handler(QLThumbnailReply(contextSize: size, currentContextDrawing: {
            (ok ? NSColor(red: 0, green: 1, blue: 0, alpha: 1) : NSColor(red: 1, green: 0, blue: 1, alpha: 1)).setFill()
            NSRect(origin: .zero, size: size).fill()
            ("S20 ." + ext as NSString).draw(at: NSPoint(x: 6, y: 6), withAttributes: [.font: NSFont.boldSystemFont(ofSize: max(10, size.height / 6)), .foregroundColor: NSColor.black])
            return true
        }), nil)
    }
}
