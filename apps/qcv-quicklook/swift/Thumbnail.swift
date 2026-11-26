// QCView's Quick Look thumbnails, the shell (canon rows 610 point 7, 612): the Rust library makes the picture
// (../src/thumb.rs - an EXR's first layer, an MXF's first frame), this file hands it to the system. No picture: an
// error, and the system draws its own icon.
import AppKit
import QuickLookThumbnailing

@_silgen_name("qlt_thumb") func qlt_thumb(_ path: UnsafePointer<CChar>, _ maxPx: UInt32, _ w: UnsafeMutablePointer<UInt32>, _ h: UnsafeMutablePointer<UInt32>) -> UnsafeMutablePointer<UInt8>?
@_silgen_name("qlt_free") func qlt_free(_ p: UnsafeMutablePointer<UInt8>?, _ w: UInt32, _ h: UInt32)

@objc(QCVThumbnail) final class QCVThumbnail: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest, _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        let none = NSError(domain: "app.qcview.quicklook", code: 2, userInfo: [NSLocalizedDescriptionKey: "no picture"])
        let px = UInt32(max(16, max(request.maximumSize.width, request.maximumSize.height) * request.scale))
        var w: UInt32 = 0, h: UInt32 = 0
        guard let p = request.fileURL.path.withCString({ qlt_thumb($0, px, &w, &h) }), w > 0, h > 0 else { handler(nil, none); return }
        let data = Data(bytes: p, count: Int(w) * Int(h) * 4) as CFData; qlt_free(p, w, h)
        guard let provider = CGDataProvider(data: data), let img = CGImage(width: Int(w), height: Int(h), bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(w) * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) else { handler(nil, none); return }
        let k = min(request.maximumSize.width / CGFloat(w), request.maximumSize.height / CGFloat(h)), fit = CGSize(width: max(1, CGFloat(w) * k), height: max(1, CGFloat(h) * k))
        handler(QLThumbnailReply(contextSize: fit, currentContextDrawing: { NSGraphicsContext.current?.cgContext.draw(img, in: CGRect(origin: .zero, size: fit)); return true }), nil)
    }
}
