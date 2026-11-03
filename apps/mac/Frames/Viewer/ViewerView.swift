import AppKit
import SwiftUI

/// One photo filling the detail area (frames_swift_spec.md 7.5): the grid's
/// thumbnail at once, then the preview, then the original on this Mac, each
/// decoded at screen size. Neighbours are prepared while you look.
struct ViewerView: View {
    @Environment(AppModel.self) private var model
    let index: Int
    @State private var image: CGImage?

    var body: some View {
        let id = model.timeline.id(at: index)
        ZoomableImage(image: image) { key in
            switch key {
            case .previous: model.step(-1)
            case .next: model.step(1)
            case .close: model.closeViewer()
            case .info: model.showInfo.toggle()
            }
        }
        .background(Color.black)
        .task(id: id) { await load(id) }
    }

    private func load(_ id: String) async {
        guard let core = model.core, let pipeline = model.pipeline, let cache = model.viewerCache else { return }
        let pixels = ViewerCache.screenPixels
        image = cache.image(for: id)
            ?? [384, 256, 128].lazy.compactMap { pipeline.cached(id, pixels: $0) }.first
            ?? pipeline.placeholder(id, hash: model.timeline.thumbhash(index))
        if cache.isFull(id) { return prepareNeighbours() }

        if let preview = await ViewerCache.decode(core: core, id: id, pixels: pixels, original: false), !Task.isCancelled {
            image = preview
            cache.store(preview, for: id, full: false)
        }
        prepareNeighbours()
        if let original = await ViewerCache.decode(core: core, id: id, pixels: pixels, original: true), !Task.isCancelled {
            image = original
            cache.store(original, for: id, full: true)
        }
    }

    private func prepareNeighbours() {
        guard let core = model.core, let cache = model.viewerCache else { return }
        for neighbour in [index + 1, index - 1] where neighbour >= 0 && neighbour < model.timeline.count {
            cache.prepare(model.timeline.id(at: neighbour), core: core)
        }
    }
}

/// Recently shown photos at screen size, so stepping back and forth is instant.
@MainActor
final class ViewerCache {
    private var images: [String: (image: CGImage, full: Bool)] = [:]
    private var order: [String] = []
    private var preparing: Set<String> = []
    private let capacity = 8

    static var screenPixels: Int {
        let screen = NSScreen.main
        let size = screen?.frame.size ?? CGSize(width: 1728, height: 1117)
        return Int(max(size.width, size.height) * (screen?.backingScaleFactor ?? 2))
    }

    func image(for id: String) -> CGImage? { images[id]?.image }
    func isFull(_ id: String) -> Bool { images[id]?.full == true }

    func store(_ image: CGImage, for id: String, full: Bool) {
        if images[id]?.full == true && !full { return }
        images[id] = (image, full)
        order.removeAll { $0 == id }
        order.append(id)
        while order.count > capacity { images[order.removeFirst()] = nil }
    }

    /// Decode a neighbour's preview ahead of time.
    func prepare(_ id: String, core: Core) {
        guard images[id] == nil, preparing.insert(id).inserted else { return }
        Task {
            if let image = await Self.decode(core: core, id: id, pixels: Self.screenPixels, original: false) {
                store(image, for: id, full: false)
            }
            preparing.remove(id)
        }
    }

    /// The preview or the original on this Mac, decoded off the main thread.
    nonisolated static func decode(core: Core, id: String, pixels: Int, original: Bool) async -> CGImage? {
        let decoded = try? await core.run { library -> DecodedImage? in
            let path = original ? library.localOriginal(assetId: id) : library.preview(assetId: id)
            return path.flatMap { ImagePipeline.decode(URL(fileURLWithPath: $0), maxPixels: pixels) }.map(DecodedImage.init)
        }
        return decoded?.cgImage
    }
}

enum ViewerKey {
    case previous, next, close, info
}

/// The photo in a magnifying scroll view: pinch or double-click to zoom,
/// arrows to move, Esc or Space to go back to the grid.
struct ZoomableImage: NSViewRepresentable {
    let image: CGImage?
    let onKey: (ViewerKey) -> Void

    func makeNSView(context: Context) -> ZoomView {
        ZoomView()
    }

    func updateNSView(_ view: ZoomView, context: Context) {
        view.onKey = onKey
        view.show(image)
    }
}

final class ZoomView: NSScrollView {
    var onKey: (ViewerKey) -> Void = { _ in }
    private let canvas = NSView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        drawsBackground = false
        hasVerticalScroller = false
        hasHorizontalScroller = false
        allowsMagnification = true
        minMagnification = 1
        maxMagnification = 8
        canvas.wantsLayer = true
        canvas.layerContentsRedrawPolicy = .never
        canvas.layer?.contentsGravity = .resizeAspect
        documentView = canvas
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var acceptsFirstResponder: Bool { true }

    func show(_ image: CGImage?) {
        guard canvas.layer?.contents as AnyObject? !== image else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        canvas.layer?.contents = image
        CATransaction.commit()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func layout() {
        super.layout()
        if magnification <= 1 { canvas.frame = CGRect(origin: .zero, size: contentView.bounds.size) }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard event.clickCount == 2 else { return super.mouseDown(with: event) }
        let point = canvas.convert(event.locationInWindow, from: nil)
        animator().setMagnification(magnification > 1 ? 1 : 2.5, centeredAt: point)
    }

    override func keyDown(with event: NSEvent) {
        switch event.specialKey {
        case .leftArrow?: onKey(.previous)
        case .rightArrow?: onKey(.next)
        default:
            if event.keyCode == 53 || event.characters == " " {
                onKey(.close)
            } else if event.charactersIgnoringModifiers == "i" && event.modifierFlags.contains(.command) {
                onKey(.info)
            } else {
                super.keyDown(with: event)
            }
        }
    }
}
