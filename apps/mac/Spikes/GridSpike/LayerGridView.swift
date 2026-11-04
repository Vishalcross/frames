import AppKit

/// The custom grid (frames_swift_spec.md 7.3): a layer-backed document view
/// that keeps a pool of CALayers for what is on screen plus the prefetch area
/// and only moves them as the view scrolls.
final class LayerGridView: NSView {
    private let library: SyntheticLibrary
    private let pipeline: ImagePipeline
    private let speed = ScrollSpeed()
    private var layout: GridLayout
    private var tiles: [Int: CALayer] = [:]
    private var tilePool: [CALayer] = []
    private var headers: [Int: CATextLayer] = [:]
    private var headerPool: [CATextLayer] = []
    private var scale: CGFloat = 2

    init(library: SyntheticLibrary, pipeline: ImagePipeline, width: CGFloat, columns: Int) {
        self.library = library
        self.pipeline = pipeline
        layout = GridLayout(library: library, width: width, columns: columns)
        super.init(frame: CGRect(x: 0, y: 0, width: width, height: layout.totalHeight))
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        speed.onSettle = { [weak self] in self?.updateVisible() }
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        scale = window?.backingScaleFactor ?? 2
        guard let clip = enclosingScrollView?.contentView else { return }
        clip.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(clipMoved), name: NSView.boundsDidChangeNotification, object: clip)
        updateVisible()
    }

    @objc private func clipMoved() {
        guard let visible = enclosingScrollView?.documentVisibleRect else { return }
        speed.update(y: visible.minY, viewport: visible.height)
        updateVisible()
    }

    func updateVisible() {
        guard let visible = enclosingScrollView?.documentVisibleRect, let root = layer else { return }
        // Prefetch two screens ahead and half a screen behind.
        let area = CGRect(x: 0, y: visible.minY - visible.height * 0.5,
                          width: visible.width, height: visible.height * 3.5)
        let (items, headerFrames) = layout.elements(in: area)
        let pixel = Int(layout.side * scale)

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        var keep = Set<Int>(minimumCapacity: items.count)
        for (index, frame) in items {
            keep.insert(index)
            let tile: CALayer
            if let existing = tiles[index] {
                tile = existing
            } else {
                tile = tilePool.popLast() ?? newTile(in: root)
                tile.isHidden = false
                tile.backgroundColor = Placeholder.colour(index)
                tile.contents = pipeline.cached(index)
                tiles[index] = tile
            }
            tile.frame = frame
            if tile.contents == nil, !speed.isFast {
                pipeline.request(index, pixelSize: pixel) { [weak self] i, image in
                    self?.tiles[i]?.contents = image
                }
            }
        }
        for (index, tile) in tiles where !keep.contains(index) {
            tile.isHidden = true
            tile.contents = nil
            tilePool.append(tile)
            tiles[index] = nil
        }

        var keepHeaders = Set<Int>()
        for (s, frame) in headerFrames {
            keepHeaders.insert(s)
            let header: CATextLayer
            if let existing = headers[s] {
                header = existing
            } else {
                header = headerPool.popLast() ?? newHeader(in: root)
                header.isHidden = false
                header.string = library.sections[s].title
                headers[s] = header
            }
            header.frame = frame.insetBy(dx: 12, dy: 12)
        }
        for (s, header) in headers where !keepHeaders.contains(s) {
            header.isHidden = true
            headerPool.append(header)
            headers[s] = nil
        }
        CATransaction.commit()
        pipeline.cancel(notIn: keep)
    }

    private func newTile(in root: CALayer) -> CALayer {
        let tile = CALayer()
        tile.contentsGravity = .resizeAspectFill
        tile.masksToBounds = true
        root.addSublayer(tile)
        return tile
    }

    private func newHeader(in root: CALayer) -> CATextLayer {
        let header = CATextLayer()
        header.fontSize = 15
        header.font = NSFont.boldSystemFont(ofSize: 15)
        header.foregroundColor = NSColor.labelColor.cgColor
        header.contentsScale = scale
        root.addSublayer(header)
        return header
    }
}
