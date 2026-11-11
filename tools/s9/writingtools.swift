// S9 step 4b — Apple's Writing Tools on the reference window's custom view (NOT an NSTextView), through
// NSWritingToolsCoordinator. Every delegate call is logged; the answers come from the same flat text and TextKit geometry
// the accessibility element uses.
//   S9_WT=limited|complete|default   attach a coordinator with that preferred behaviour (unset: no coordinator)
//   S9_WT_DO=<menu title>            after S9_WT_AT seconds (default 3) select the first two paragraphs and invoke the
//                                    Writing Tools menu item of that title ("show" = showWritingTools:) — the app acting
//                                    on itself, no input is sent
//   S9_SECONDS=N                     quit after N seconds
import Cocoa

let wtMode = ProcessInfo.processInfo.environment["S9_WT"]

@available(macOS 15.2, *)
final class WT: NSObject, NSWritingToolsCoordinator.Delegate {
    unowned let v: EditorView
    var coordinator: NSWritingToolsCoordinator!
    /// context id → where its attributed string starts in the document
    var origin: [UUID: Int] = [:]
    init(view: EditorView) {
        v = view; super.init()
        coordinator = NSWritingToolsCoordinator(delegate: self)
        coordinator.preferredBehavior = wtMode == "limited" ? .limited : wtMode == "complete" ? .complete : .default
        coordinator.preferredResultOptions = .plainText
        view.writingToolsCoordinator = coordinator
        Log.w("WT.attach", "available \(NSWritingToolsCoordinator.isWritingToolsAvailable); preferred \(wtMode ?? "") → behavior \(coordinator.behavior.rawValue) (−1 none, 1 complete, 2 limited); resultOptions \(coordinator.resultOptions.rawValue)", always: true)
    }
    func doc(_ r: NSRange, _ c: NSWritingToolsCoordinator.Context) -> NSRange {
        let o = origin[c.identifier] ?? 0
        let loc = min(v.length, o + r.location); return NSRange(location: loc, length: min(r.length, v.length - loc))
    }
    func scopeName(_ s: NSWritingToolsCoordinator.ContextScope) -> String { switch s { case .userSelection: return "userSelection"; case .fullDocument: return "fullDocument"; case .visibleArea: return "visibleArea"; @unknown default: return "scope \(s.rawValue)" } }
    func animName(_ a: NSWritingToolsCoordinator.TextAnimation) -> String { ["anticipate", "remove", "insert"].indices.contains(a.rawValue) ? ["anticipate", "remove", "insert"][a.rawValue] : "animation \(a.rawValue)" }

    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, requestsContextsFor scope: NSWritingToolsCoordinator.ContextScope, completion: @escaping ([NSWritingToolsCoordinator.Context]) -> Void) {
        // The context is the paragraphs around the selection (the whole text for fullDocument, the visible lines for visibleArea).
        var span: NSRange, inner: NSRange
        switch scope {
        case .fullDocument: span = NSRange(location: 0, length: v.length); inner = v.sel
        case .visibleArea: span = v.text.paragraphRange(for: v.visibleChars()); inner = NSRange(location: 0, length: span.length); inner.location = span.location
        default: span = v.text.paragraphRange(for: v.sel); inner = v.sel
        }
        let ctx = NSWritingToolsCoordinator.Context(attributedString: v.storage.attributedSubstring(from: span), range: NSRange(location: inner.location - span.location, length: inner.length))
        origin[ctx.identifier] = span.location
        Log.w("WT.contexts", "\(scopeName(scope)): context \(R(span)) of the document, range in it \(R(ctx.range)); selection \(R(v.sel))", always: true)
        completion([ctx])
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, replace range: NSRange, in context: NSWritingToolsCoordinator.Context, proposedText: NSAttributedString, reason: NSWritingToolsCoordinator.TextReplacementReason, animationParameters: NSWritingToolsCoordinator.AnimationParameters?, completion: @escaping (NSAttributedString?) -> Void) {
        let d = doc(range, context)
        Log.w("WT.replace", "\(R(range)) in context = \(R(d)) \"\(Log.clip(v.text.substring(with: d)))\" → \"\(Log.clip(proposedText.string))\" reason \(reason.rawValue) (0 interactive, 1 noninteractive, 2 accepted, 3 rejected, 4 temporary) animated \(animationParameters != nil); resolvedRange \(R(context.resolvedRange))", always: true)
        v.replace(d, with: proposedText.string, why: "WT.replace")
        completion(NSAttributedString(string: proposedText.string, attributes: v.attrs))
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, select ranges: [NSValue], in context: NSWritingToolsCoordinator.Context, completion: @escaping () -> Void) {
        Log.w("WT.select", "\(ranges.map { R($0.rangeValue) })", always: true)
        if let r = ranges.first?.rangeValue { let d = doc(r, context); v.anchor = d.location; v.select(d, why: "WT") }
        completion()
    }
    func rects(_ d: NSRange) -> [NSRect] {   // view coordinates, one per line
        var out: [NSRect] = []
        let g = v.layout.glyphRange(forCharacterRange: d, actualCharacterRange: nil)
        v.layout.enumerateEnclosingRects(forGlyphRange: g, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: v.container) { r, _ in out.append(r.offsetBy(dx: self.v.inset, dy: self.v.inset)) }
        return out
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, requestsBoundingBezierPathsFor range: NSRange, in context: NSWritingToolsCoordinator.Context, completion: @escaping ([NSBezierPath]) -> Void) {
        let r = rects(doc(range, context)); Log.w("WT.boundingPaths", "\(R(range)) → \(r.count) rects")
        completion(r.map { NSBezierPath(rect: $0) })
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, requestsUnderlinePathsFor range: NSRange, in context: NSWritingToolsCoordinator.Context, completion: @escaping ([NSBezierPath]) -> Void) {
        let r = rects(doc(range, context)); Log.w("WT.underlinePaths", "\(R(range)) → \(r.count) rects")
        completion(r.map { NSBezierPath(rect: NSRect(x: $0.minX, y: $0.maxY - 2, width: $0.width, height: 2)) })
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, prepareFor textAnimation: NSWritingToolsCoordinator.TextAnimation, for range: NSRange, in context: NSWritingToolsCoordinator.Context, completion: @escaping () -> Void) {
        Log.w("WT.prepareAnimation", "\(animName(textAnimation)) \(R(range))", always: true); completion()
    }
    func preview(_ rect: NSRect) -> NSTextPreview? {
        guard rect.width > 0, rect.height > 0, let rep = v.bitmapImageRepForCachingDisplay(in: rect) else { return nil }
        v.cacheDisplay(in: rect, to: rep)
        guard let img = rep.cgImage else { return nil }
        return NSTextPreview(snapshotImage: img, presentationFrame: rect)
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, requestsPreviewFor textAnimation: NSWritingToolsCoordinator.TextAnimation, of range: NSRange, in context: NSWritingToolsCoordinator.Context, completion: @escaping ([NSTextPreview]?) -> Void) {
        let d = doc(range, context); Log.w("WT.previewForAnimation", "\(animName(textAnimation)) \(R(range))", always: true)
        completion(preview(v.rect(for: d)).map { [$0] })
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, requestsPreviewFor rect: NSRect, in context: NSWritingToolsCoordinator.Context, completion: @escaping (NSTextPreview?) -> Void) {
        Log.w("WT.previewForRect", "(\(Int(rect.minX)),\(Int(rect.minY)) \(Int(rect.width))×\(Int(rect.height)))", always: true); completion(preview(rect))
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, finish textAnimation: NSWritingToolsCoordinator.TextAnimation, for range: NSRange, in context: NSWritingToolsCoordinator.Context, completion: @escaping () -> Void) {
        Log.w("WT.finishAnimation", "\(animName(textAnimation)) \(R(range))", always: true); completion()
    }
    func writingToolsCoordinator(_ c: NSWritingToolsCoordinator, willChangeTo newState: NSWritingToolsCoordinator.State, completion: @escaping () -> Void) {
        Log.w("WT.state", "\(c.state.rawValue) → \(newState.rawValue) (0 inactive, 1 noninteractive, 2 interactive resting, 3 interactive streaming)", always: true); completion()
    }
}

/// The Writing Tools menu as the system builds it, logged (titles, actions), and one item invoked by title.
func wtMenuItems() -> [NSMenuItem] { if #available(macOS 15.2, *) { return NSMenuItem.writingToolsItems }; return [] }
func wtLogMenu(_ items: [NSMenuItem], depth: Int = 0) {
    for i in items {
        Log.w("WT.menu", String(repeating: "  ", count: depth) + (i.isSeparatorItem ? "—" : "\"\(i.title)\" action \(i.action.map { NSStringFromSelector($0) } ?? "nil") tag \(i.tag) enabled \(i.isEnabled)"), always: true)
        if let s = i.submenu { wtLogMenu(s.items, depth: depth + 1) }
    }
}
func wtFind(_ title: String, in items: [NSMenuItem]) -> NSMenuItem? {
    for i in items { if i.title == title { return i }; if let s = i.submenu, let f = wtFind(title, in: s.items) { return f } }
    return nil
}
