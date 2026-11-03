import AppKit
import SwiftUI

/// One photo filling the detail area (frames_swift_spec.md 7.5): the grid's
/// thumbnail at once, then the preview, then the original on this Mac, each
/// decoded at screen size. Zooming past 100% brings in the full-resolution
/// original. Neighbours are prepared while you look.
struct ViewerView: View {
    @Environment(AppModel.self) private var model
    let index: Int
    @State private var image: CGImage?
    @State private var fullResolution: CGImage?

    var body: some View {
        let id = model.timeline.id(at: index)
        let opening = model.transitions.phase == .opening
        ZoomableImage(assetID: id, image: fullResolution ?? image, hidden: opening, transitions: model.transitions) { key in
            switch key {
            case .previous: model.step(-1)
            case .next: model.step(1)
            case .close: model.closeViewer()
            case .info: model.showInfo.toggle()
            case .fullResolution: Task { await loadFullResolution(id) }
            }
        }
        .background(opening ? Color.clear : Color.black)
        .task(id: id) {
            fullResolution = nil
            await load(id)
        }
    }

    private func load(_ id: String) async {
        guard let core = model.core, let pipeline = model.pipeline, let cache = model.viewerCache else { return }
        let pixels = ViewerCache.screenPixels
        image = cache.image(for: id)
            ?? [512, 384, 256, 128].lazy.compactMap { pipeline.cached(id, pixels: $0) }.first
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

    /// The original at its own size, for detail past 100%. Not cached: one
    /// at a time, dropped when you move on.
    private func loadFullResolution(_ id: String) async {
        guard let core = model.core, fullResolution == nil else { return }
        let full = await ViewerCache.decode(core: core, id: id, pixels: 16_384, original: true)
        if model.viewerIndex.map({ model.timeline.id(at: $0) }) == id { fullResolution = full }
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
    case previous, next, close, info, fullResolution
}

/// The photo in a magnifying scroll view: pinch or double-click to zoom,
/// arrows to move, Esc or Space to go back to the grid.
struct ZoomableImage: NSViewRepresentable {
    let assetID: String
    let image: CGImage?
    let hidden: Bool
    let transitions: ViewerTransitions
    let onKey: (ViewerKey) -> Void

    func makeNSView(context: Context) -> ZoomView {
        ZoomView()
    }

    func updateNSView(_ view: ZoomView, context: Context) {
        view.onKey = onKey
        view.transitions = transitions
        view.show(image, assetID: assetID, hidden: hidden)
    }
}

final class ZoomView: NSScrollView {
    var onKey: (ViewerKey) -> Void = { _ in }
    weak var transitions: ViewerTransitions?
    private(set) var image: CGImage?
    private let canvas = NSView()
    private var assetID: String?
    private var askedForFullResolution = false
    private var appeared = false

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
        NotificationCenter.default.addObserver(self, selector: #selector(checkMagnification),
                                               name: NSScrollView.didEndLiveMagnifyNotification, object: self)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var acceptsFirstResponder: Bool { true }

    func show(_ image: CGImage?, assetID: String, hidden: Bool) {
        if assetID != self.assetID {
            self.assetID = assetID
            askedForFullResolution = false
            if magnification != 1 { magnification = 1 }
        }
        canvas.isHidden = hidden
        guard self.image !== image else { return }
        self.image = image
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        canvas.layer?.contents = image
        CATransaction.commit()
    }

    override var magnification: CGFloat {
        didSet { checkMagnification() }
    }

    /// Past 100% the screen-size image runs out of detail.
    @objc private func checkMagnification() {
        if magnification > 1.05, !askedForFullResolution {
            askedForFullResolution = true
            onKey(.fullResolution)
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func layout() {
        super.layout()
        if magnification <= 1 { canvas.frame = CGRect(origin: .zero, size: contentView.bounds.size) }
        if !appeared, window != nil, bounds.width > 0 {
            appeared = true
            transitions?.viewerAppeared(self)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard event.clickCount == 2 else { return super.mouseDown(with: event) }
        let point = canvas.convert(event.locationInWindow, from: nil)
        animator().setMagnification(magnification > 1 ? 1 : 2.5, centeredAt: point)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in self?.checkMagnification() }
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
