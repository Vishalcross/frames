import AppKit

/// The photo grid (frames_swift_spec.md 7.3): a layer-backed document view
/// that keeps a pool of layers for what is on screen and the prefetch area,
/// and only moves them as the view scrolls. Nothing is allocated per frame.
@MainActor
final class GridView: NSView {
    var onOpen: (Int) -> Void = { _ in }
    var onSelect: (Int?) -> Void = { _ in }
    var onColumnsChange: (Int) -> Void = { _ in }

    private let pipeline: ImagePipeline
    private var timeline = Timeline.empty
    private var layout = GridLayout(sections: [], width: 0, columns: 7)
    private(set) var columns = 7
    private(set) var selection: Int?
    private var tiles: [Int: TileLayer] = [:]
    private var tilePool: [TileLayer] = []
    private var headers: [Int: CATextLayer] = [:]
    private var headerPool: [CATextLayer] = []
    private let stickyHeader = HeaderBar()
    private let speed = ScrollSpeed()
    private var magnification: CGFloat = 0

    init(pipeline: ImagePipeline) {
        self.pipeline = pipeline
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        layer?.addSublayer(stickyHeader)
        stickyHeader.zPosition = 10
        speed.onSettle = { [weak self] in self?.updateVisible() }
        NotificationCenter.default.addObserver(
            self, selector: #selector(thumbnailArrived(_:)), name: .thumbnailArrived, object: nil)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    // MARK: Contents

    func show(_ timeline: Timeline, columns: Int) {
        let changed = timeline.version != self.timeline.version
        let selectedID = selection.flatMap { $0 < self.timeline.count ? self.timeline.id(at: $0) : nil }
        self.timeline = timeline
        if changed {
            recycleAll()
            selection = selectedID.flatMap { timeline.index(of: $0) }
        }
        if changed || columns != self.columns {
            setColumns(columns, anchor: nil)
        }
    }

    func select(_ index: Int?, reveal: Bool = true) {
        guard index != selection, index == nil || timeline.count > 0 else { return }
        let old = selection
        selection = index.map { min(max(0, $0), timeline.count - 1) }
        for i in [old, selection].compactMap({ $0 }) { tiles[i]?.isSelected = i == selection }
        if reveal, let selection { scrollToVisible(frame(of: selection).insetBy(dx: 0, dy: -GridLayout.headerHeight)) }
        onSelect(selection)
    }

    /// Bring `index` to the top of the view, under the sticky header.
    func scroll(toTop index: Int) {
        guard let s = timeline.section(containing: index), let clip = enclosingScrollView?.contentView else { return }
        let y = index == timeline.sections[s].start ? layout.sectionTop(s) : frame(of: index).minY - GridLayout.headerHeight
        clip.scroll(to: NSPoint(x: 0, y: max(0, min(y, frame.height - clip.bounds.height))))
        enclosingScrollView?.reflectScrolledClipView(clip)
    }

    func frame(of index: Int) -> CGRect {
        guard let s = timeline.section(containing: index) else { return .zero }
        return layout.frame(of: index, inSection: s)
    }

    // MARK: Layout

    /// Change the column count, keeping the tile under `anchor` (in this
    /// view's coordinates) where it is on screen, or the top tile if none.
    func setColumns(_ count: Int, anchor: CGPoint?) {
        guard let clip = enclosingScrollView?.contentView else {
            columns = count
            return
        }
        let visible = clip.bounds
        let anchorPoint = anchor ?? CGPoint(x: visible.midX, y: visible.minY + GridLayout.headerHeight + 1)
        let anchorIndex = layout.index(at: anchorPoint) ?? layout.elements(in: visible).tiles.first?.index
        let offsetOnScreen = anchorIndex.map { frame(of: $0).minY - visible.minY } ?? 0

        columns = count
        layout = GridLayout(sections: timeline.sections, width: clip.bounds.width, columns: count)
        setFrameSize(NSSize(width: clip.bounds.width, height: max(layout.totalHeight, clip.bounds.height)))
        recycleAll()
        if let anchorIndex, timeline.count > 0 {
            let y = frame(of: anchorIndex).minY - offsetOnScreen
            clip.scroll(to: NSPoint(x: 0, y: max(0, min(y, frame.height - visible.height))))
            enclosingScrollView?.reflectScrolledClipView(clip)
        }
        updateVisible()
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        guard let clip = enclosingScrollView?.contentView else { return }
        clip.postsBoundsChangedNotifications = true
        clip.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(self, selector: #selector(clipScrolled), name: NSView.boundsDidChangeNotification, object: clip)
        NotificationCenter.default.addObserver(self, selector: #selector(clipResized), name: NSView.frameDidChangeNotification, object: clip)
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        let scale = window?.backingScaleFactor ?? 2
        stickyHeader.contentsScale = scale
        for header in headers.values { header.contentsScale = scale }
        recycleAll()
        updateVisible()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        effectiveAppearance.performAsCurrentDrawingAppearance {
            stickyHeader.applyColours()
            for header in headers.values { header.foregroundColor = NSColor.labelColor.cgColor }
            for tile in tiles.values { tile.applyColours() }
        }
    }

    @objc private func clipScrolled() {
        guard let visible = enclosingScrollView?.documentVisibleRect else { return }
        speed.update(y: visible.minY, viewport: visible.height)
        updateVisible()
    }

    @objc private func clipResized() {
        guard let clip = enclosingScrollView?.contentView, abs(clip.bounds.width - layout.width) > 0.5 else { return }
        setColumns(columns, anchor: nil)
    }

    @objc private func thumbnailArrived(_ note: Notification) {
        guard let id = note.object as? String else { return }
        for (index, tile) in tiles where timeline.id(at: index) == id {
            load(tile, index: index)
        }
    }

    /// Place layers for everything on screen plus two screens ahead and half
    /// a screen behind, and recycle the rest.
    func updateVisible() {
        guard let visible = enclosingScrollView?.documentVisibleRect, let root = layer, timeline.count > 0 else {
            stickyHeader.isHidden = true
            return
        }
        let area = CGRect(x: 0, y: visible.minY - visible.height * 0.5, width: visible.width, height: visible.height * 3.5)
        let (visibleTiles, visibleHeaders) = layout.elements(in: area)

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        var keep = Set<Int>(minimumCapacity: visibleTiles.count)
        var wanted = Set<String>(minimumCapacity: visibleTiles.count)
        let pixels = Int(layout.side * (window?.backingScaleFactor ?? 2))
        for (index, frame) in visibleTiles {
            keep.insert(index)
            let tile: TileLayer
            if let existing = tiles[index] {
                tile = existing
            } else {
                tile = tilePool.popLast() ?? newTile(in: root)
                tile.isHidden = false
                configure(tile, index: index)
                tiles[index] = tile
            }
            tile.frame = frame
            tile.layoutBadges()
            if !tile.hasImage {
                wanted.insert(ImagePipeline.key(timeline.id(at: index), pixels: pixels))
                if !speed.isFast { load(tile, index: index) }
            }
        }
        for (index, tile) in tiles where !keep.contains(index) {
            tile.isHidden = true
            tile.reset()
            tilePool.append(tile)
            tiles[index] = nil
        }

        var keepHeaders = Set<Int>()
        for (section, frame) in visibleHeaders {
            keepHeaders.insert(section)
            let header: CATextLayer
            if let existing = headers[section] {
                header = existing
            } else {
                header = headerPool.popLast() ?? newHeader(in: root)
                header.isHidden = false
                header.string = timeline.sections[section].title
                headers[section] = header
            }
            header.frame = frame.insetBy(dx: 14, dy: 12)
        }
        for (section, header) in headers where !keepHeaders.contains(section) {
            header.isHidden = true
            headerPool.append(header)
            headers[section] = nil
        }

        // The month at the top stays pinned until the next one pushes it up.
        let top = layout.section(atOrBefore: visible.minY)
        let next = top + 1 < timeline.sections.count ? layout.sectionTop(top + 1) : .infinity
        stickyHeader.isHidden = visible.minY <= layout.sectionTop(top)
        stickyHeader.title = timeline.sections[top].title
        stickyHeader.frame = CGRect(x: 0, y: min(visible.minY, next - GridLayout.headerHeight), width: visible.width, height: GridLayout.headerHeight)
        CATransaction.commit()

        pipeline.cancel(keeping: wanted)
        #if DEBUG
        DebugHooks.mark("first grid frame")
        #endif
    }

    private func configure(_ tile: TileLayer, index: Int) {
        let id = timeline.id(at: index)
        let pixels = Int(layout.side * (window?.backingScaleFactor ?? 2))
        if let image = pipeline.cached(id, pixels: pixels) {
            tile.setImage(image)
        } else {
            tile.setPlaceholder(pipeline.placeholder(id, hash: timeline.thumbhash(index)))
        }
        tile.videoSeconds = timeline.isVideo(index) ? timeline.durationSeconds(index) : nil
        tile.isLive = timeline.isLive(index)
        tile.isSelected = index == selection
    }

    private func load(_ tile: TileLayer, index: Int) {
        let pixels = Int(layout.side * (window?.backingScaleFactor ?? 2))
        pipeline.request(timeline.id(at: index), pixels: pixels) { [weak self, weak tile] image in
            guard let self, let tile, self.tiles[index] === tile else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            tile.setImage(image)
            CATransaction.commit()
        }
    }

    private func recycleAll() {
        for (_, tile) in tiles {
            tile.isHidden = true
            tile.reset()
            tilePool.append(tile)
        }
        tiles.removeAll()
        for (_, header) in headers {
            header.isHidden = true
            headerPool.append(header)
        }
        headers.removeAll()
    }

    private func newTile(in root: CALayer) -> TileLayer {
        let tile = TileLayer()
        tile.contentsScale = window?.backingScaleFactor ?? 2
        effectiveAppearance.performAsCurrentDrawingAppearance { tile.applyColours() }
        root.insertSublayer(tile, below: stickyHeader)
        return tile
    }

    private func newHeader(in root: CALayer) -> CATextLayer {
        let header = CATextLayer()
        header.font = NSFont.systemFont(ofSize: 17, weight: .bold)
        header.fontSize = 17
        header.contentsScale = window?.backingScaleFactor ?? 2
        effectiveAppearance.performAsCurrentDrawingAppearance { header.foregroundColor = NSColor.labelColor.cgColor }
        root.insertSublayer(header, below: stickyHeader)
        return header
    }

    // MARK: Input

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        guard let index = layout.index(at: point) else {
            select(nil, reveal: false)
            return
        }
        select(index, reveal: false)
        if event.clickCount == 2 { onOpen(index) }
    }

    override func keyDown(with event: NSEvent) {
        let current = selection ?? -1
        switch event.specialKey {
        case .leftArrow?: select(max(0, current - 1))
        case .rightArrow?: select(min(timeline.count - 1, current + 1))
        case .upArrow?: select(max(0, current - columns))
        case .downArrow?: select(min(timeline.count - 1, current + columns))
        case .carriageReturn?, .enter?:
            if let selection { onOpen(selection) }
        default:
            if event.characters == " ", let selection {
                onOpen(selection)
            } else {
                super.keyDown(with: event)
            }
        }
    }

    /// Pinch to change the column count, keeping the tile under the fingers in place.
    override func magnify(with event: NSEvent) {
        magnification += event.magnification
        if event.phase == .ended || event.phase == .cancelled { magnification = 0 }
        let step: Int
        if magnification > 0.15 {
            step = -1
        } else if magnification < -0.15 {
            step = 1
        } else {
            return
        }
        magnification = 0
        let count = min(15, max(3, columns + step))
        guard count != columns else { return }
        setColumns(count, anchor: convert(event.locationInWindow, from: nil))
        onColumnsChange(count)
    }
}

/// One tile: the image as the layer's contents, plus badges and a selection ring.
final class TileLayer: CALayer {
    private(set) var hasImage = false
    private var badge: CATextLayer?
    private var liveBadge: CATextLayer?

    /// A video's length, shown as a badge.
    var videoSeconds: Int? {
        didSet {
            guard videoSeconds != oldValue else { return }
            if let seconds = videoSeconds {
                let label = badge ?? makeBadge()
                badge = label
                label.string = String(format: "%d:%02d", seconds / 60, seconds % 60)
                label.isHidden = false
            } else {
                badge?.isHidden = true
            }
        }
    }

    var isLive = false {
        didSet {
            guard isLive != oldValue else { return }
            if isLive {
                let label = liveBadge ?? makeBadge()
                liveBadge = label
                label.string = "LIVE"
                label.isHidden = false
            } else {
                liveBadge?.isHidden = true
            }
        }
    }

    var isSelected = false {
        didSet { borderWidth = isSelected ? 3 : 0 }
    }

    override init() {
        super.init()
        contentsGravity = .resizeAspectFill
        masksToBounds = true
    }

    override init(layer: Any) { super.init(layer: layer) }
    required init?(coder: NSCoder) { fatalError("not used") }

    func setImage(_ image: CGImage) {
        contents = image
        hasImage = true
    }

    func setPlaceholder(_ image: CGImage?) {
        contents = image
        hasImage = false
    }

    func reset() {
        contents = nil
        hasImage = false
        videoSeconds = nil
        isLive = false
        isSelected = false
    }

    func applyColours() {
        backgroundColor = NSColor.quaternaryLabelColor.cgColor
        borderColor = NSColor.controlAccentColor.cgColor
    }

    func layoutBadges() {
        if let badge, !badge.isHidden {
            badge.frame = CGRect(x: bounds.width - 46, y: bounds.height - 20, width: 42, height: 16)
        }
        if let liveBadge, !liveBadge.isHidden {
            liveBadge.frame = CGRect(x: 4, y: 4, width: 34, height: 16)
        }
    }

    private func makeBadge() -> CATextLayer {
        let label = CATextLayer()
        label.font = NSFont.systemFont(ofSize: 11, weight: .semibold)
        label.fontSize = 11
        label.alignmentMode = .center
        label.foregroundColor = NSColor.white.cgColor
        label.backgroundColor = NSColor.black.withAlphaComponent(0.45).cgColor
        label.cornerRadius = 4
        label.contentsScale = contentsScale
        addSublayer(label)
        return label
    }
}

/// The month title pinned at the top of the grid while its photos scroll under it.
final class HeaderBar: CALayer {
    private let label = CATextLayer()

    var title: String = "" {
        didSet { if title != oldValue { label.string = title } }
    }

    override init() {
        super.init()
        label.font = NSFont.systemFont(ofSize: 17, weight: .bold)
        label.fontSize = 17
        addSublayer(label)
        isHidden = true
    }

    override init(layer: Any) { super.init(layer: layer) }
    required init?(coder: NSCoder) { fatalError("not used") }

    override var contentsScale: CGFloat {
        didSet { label.contentsScale = contentsScale }
    }

    override func layoutSublayers() {
        super.layoutSublayers()
        label.frame = bounds.insetBy(dx: 14, dy: 12)
    }

    func applyColours() {
        backgroundColor = NSColor.windowBackgroundColor.withAlphaComponent(0.92).cgColor
        label.foregroundColor = NSColor.labelColor.cgColor
    }
}

/// Scroll speed in screens per second. Above 3, tiles keep their blurred
/// sketches and decoding waits until the scroll settles (spec 7.4).
@MainActor
final class ScrollSpeed {
    private(set) var isFast = false
    var onSettle: () -> Void = {}
    private var lastY: CGFloat = 0
    private var lastTime: CFTimeInterval = 0
    private var speed: Double = 0
    private var settleTimer: Timer?

    func update(y: CGFloat, viewport: CGFloat) {
        let now = CACurrentMediaTime()
        let dt = now - lastTime
        if lastTime > 0, dt > 0, dt < 0.25, viewport > 0 {
            speed = speed * 0.7 + Double(abs(y - lastY)) / Double(viewport) / dt * 0.3
        }
        lastY = y
        lastTime = now
        isFast = speed > 3
        settleTimer?.invalidate()
        settleTimer = Timer.scheduledTimer(withTimeInterval: 0.12, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.speed = 0
                self.isFast = false
                self.onSettle()
            }
        }
    }
}
