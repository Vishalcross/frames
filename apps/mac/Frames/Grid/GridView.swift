import AppKit

/// The photo grid (frames_swift_spec.md 7.3): a layer-backed document view
/// that keeps a pool of layers for what is on screen and the prefetch area,
/// and only moves them as the view scrolls. Nothing is allocated per frame.
@MainActor
final class GridView: NSView, NSDraggingSource {
    var onOpen: (Int) -> Void = { _ in }
    var onSelectionChange: (IndexSet, Int?) -> Void = { _, _ in }
    var onColumnsChange: (Int) -> Void = { _ in }
    /// The right-click menu for the current selection.
    var menuProvider: () -> NSMenu? = { nil }
    /// Original file names for the dragged ids, in order.
    var filenames: ([String]) -> [String] = { _ in [] }
    var promiseWriter: AssetPromiseWriter?
    var onScroll: () -> Void = {}
    /// The photo browsing rests on: under the pointer for 0.4 s, or in the
    /// middle of the view when a scroll settles. Adaptive takes its colours.
    var onBrowse: (Int) -> Void = { _ in }
    /// Selection rings and check marks; the system accent unless the theme sets one.
    var accent: NSColor? {
        didSet { if accent != oldValue { recolour() } }
    }
    /// The pinned month header's background; the window's unless the theme tints it.
    var canvas: NSColor? {
        didSet { if canvas != oldValue { recolour() } }
    }
    private var hoverTimer: Timer?
    private var hoverIndex: Int?

    private let pipeline: ImagePipeline
    private(set) var timeline = Timeline.empty
    private var layout = GridLayout(sections: [], width: 0, columns: 7)
    private(set) var columns = 7
    private(set) var selection = IndexSet()
    private(set) var focus: Int?
    private var anchor: Int?
    private var tiles: [Int: TileLayer] = [:]
    private var tilePool: [TileLayer] = []
    private var headers: [Int: CATextLayer] = [:]
    private var headerPool: [CATextLayer] = []
    private let stickyHeader = HeaderBar()
    private let marquee = CAShapeLayer()
    private let speed = ScrollSpeed()
    private var mouseDownPoint: CGPoint?
    private var mouseDownIndex: Int?
    private var marqueeBase: IndexSet?
    private var pinchScale: CGFloat = 1
    private var pinchAnchor: CGPoint = .zero
    private var pinchFrames: [Int: CGRect] = [:]

    init(pipeline: ImagePipeline) {
        self.pipeline = pipeline
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        layer?.addSublayer(stickyHeader)
        stickyHeader.zPosition = 10
        marquee.zPosition = 20
        marquee.lineWidth = 1
        marquee.isHidden = true
        layer?.addSublayer(marquee)
        speed.onSettle = { [weak self] in
            self?.updateVisible()
            self?.browseMiddle()
        }
        NotificationCenter.default.addObserver(
            self, selector: #selector(thumbnailArrived(_:)), name: .thumbnailArrived, object: nil)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                       owner: self, userInfo: nil))
    }

    override func mouseMoved(with event: NSEvent) {
        let index = layout.index(at: convert(event.locationInWindow, from: nil))
        guard index != hoverIndex else { return }
        hoverIndex = index
        hoverTimer?.invalidate()
        guard let index else { return }
        hoverTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.hoverIndex == index else { return }
                self.onBrowse(index)
            }
        }
    }

    override func mouseExited(with event: NSEvent) {
        hoverIndex = nil
        hoverTimer?.invalidate()
    }

    private func browseMiddle() {
        guard let visible = enclosingScrollView?.documentVisibleRect,
              let index = layout.index(at: CGPoint(x: visible.midX, y: visible.midY)) else { return }
        onBrowse(index)
    }

    private func recolour() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            stickyHeader.applyColours(background: canvas)
            for tile in tiles.values { tile.applyColours(accent: accent) }
            for tile in tilePool { tile.applyColours(accent: accent) }
            let ring = accent ?? NSColor.controlAccentColor
            marquee.strokeColor = ring.cgColor
            marquee.fillColor = ring.withAlphaComponent(0.15).cgColor
        }
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    // MARK: Contents

    func show(_ timeline: Timeline, columns: Int) {
        let changed = timeline.version != self.timeline.version
        self.timeline = timeline
        if changed {
            recycleAll()
            setColumns(columns, anchor: nil, animated: false)
        } else if columns != self.columns {
            setColumns(columns, anchor: nil, animated: true)
        }
    }

    /// Show `selection` without reporting it back.
    func setSelection(_ selection: IndexSet, focus: Int?) {
        guard selection != self.selection || focus != self.focus else { return }
        let changed = self.selection.symmetricDifference(selection)
        self.selection = selection
        self.focus = focus
        if anchor == nil || !(selection.contains(anchor ?? -1)) { anchor = focus }
        for index in changed { tiles[index]?.isSelected = selection.contains(index) }
    }

    private func select(_ newSelection: IndexSet, focus newFocus: Int?, reveal: Bool) {
        setSelection(newSelection, focus: newFocus)
        if reveal, let newFocus { scrollToVisible(frame(of: newFocus).insetBy(dx: 0, dy: -GridLayout.headerHeight)) }
        onSelectionChange(selection, focus)
    }

    /// Bring `index` to the top of the view, under the sticky header.
    func scroll(toTop index: Int) {
        guard let s = timeline.section(containing: index), let clip = enclosingScrollView?.contentView else { return }
        let y = index == timeline.sections[s].start ? layout.sectionTop(s) : frame(of: index).minY - GridLayout.headerHeight
        scroll(toY: y, in: clip)
    }

    /// Make sure `index` is on screen, scrolling as little as possible.
    func reveal(_ index: Int) {
        guard index < timeline.count else { return }
        scrollToVisible(frame(of: index).insetBy(dx: 0, dy: -GridLayout.headerHeight))
        updateVisible()
    }

    func frame(of index: Int) -> CGRect {
        guard let s = timeline.section(containing: index) else { return .zero }
        return layout.frame(of: index, inSection: s)
    }

    /// Where a tile is in window coordinates, and what it shows, if it is on screen.
    func tileSnapshot(_ index: Int) -> (frame: CGRect, image: CGImage?)? {
        guard let tile = tiles[index], let visible = enclosingScrollView?.documentVisibleRect,
              visible.intersects(tile.frame) else { return nil }
        return (convert(tile.frame, to: nil), tile.contents.map { $0 as! CGImage })
    }

    // MARK: Year scrubber

    /// Each year's first month, as a fraction of the scrollable height.
    var yearMarks: [(year: Int, fraction: CGFloat)] {
        guard let clip = enclosingScrollView?.contentView else { return [] }
        let range = max(1, frame.height - clip.bounds.height)
        var marks: [(year: Int, fraction: CGFloat)] = []
        for (s, section) in timeline.sections.enumerated() where section.heading == nil && marks.last?.year != section.year {
            marks.append((section.year, min(1, layout.sectionTop(s) / range)))
        }
        return marks
    }

    var scrollFraction: CGFloat {
        guard let clip = enclosingScrollView?.contentView else { return 0 }
        return clip.bounds.minY / max(1, frame.height - clip.bounds.height)
    }

    func scroll(toFraction fraction: CGFloat) {
        guard let clip = enclosingScrollView?.contentView else { return }
        scroll(toY: fraction * (frame.height - clip.bounds.height), in: clip)
    }

    func monthTitle(atFraction fraction: CGFloat) -> String {
        guard let clip = enclosingScrollView?.contentView, !timeline.sections.isEmpty else { return "" }
        let y = fraction * (frame.height - clip.bounds.height)
        return timeline.sections[layout.section(atOrBefore: y)].title
    }

    private func scroll(toY y: CGFloat, in clip: NSClipView) {
        clip.scroll(to: NSPoint(x: 0, y: max(0, min(y, frame.height - clip.bounds.height))))
        enclosingScrollView?.reflectScrolledClipView(clip)
    }

    // MARK: Layout

    /// Change the column count, keeping the tile under `anchor` (in this
    /// view's coordinates) where it is on screen, or the top tile if none.
    /// Animated, tiles glide from their old places to their new ones.
    func setColumns(_ count: Int, anchor: CGPoint?, animated: Bool) {
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
        if let anchorIndex, timeline.count > 0 {
            scroll(toY: frame(of: anchorIndex).minY - offsetOnScreen, in: clip)
        }
        guard animated, !tiles.isEmpty else {
            recycleAll()
            updateVisible()
            return
        }
        // Existing tiles move to their new places; new ones fade in; the
        // ones that left are recycled once the movement ends.
        CATransaction.begin()
        CATransaction.setAnimationDuration(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion ? 0 : 0.28)
        CATransaction.setAnimationTimingFunction(CAMediaTimingFunction(name: .easeInEaseOut))
        CATransaction.setCompletionBlock { [weak self] in self?.updateVisible() }
        for (index, tile) in tiles {
            tile.frame = frame(of: index)
            tile.layoutBadges()
        }
        for (_, header) in headers { header.opacity = 0 }
        CATransaction.commit()
        updateVisible(fadeIn: true)
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
            for header in headers.values { header.foregroundColor = NSColor.labelColor.cgColor }
        }
        recolour()
    }

    @objc private func clipScrolled() {
        guard let visible = enclosingScrollView?.documentVisibleRect else { return }
        speed.update(y: visible.minY, viewport: visible.height)
        updateVisible()
        onScroll()
    }

    @objc private func clipResized() {
        guard let clip = enclosingScrollView?.contentView, abs(clip.bounds.width - layout.width) > 0.5 else { return }
        setColumns(columns, anchor: nil, animated: false)
    }

    @objc private func thumbnailArrived(_ note: Notification) {
        guard let id = note.object as? String else { return }
        for (index, tile) in tiles where timeline.id(at: index) == id {
            load(tile, index: index)
        }
    }

    /// Place layers for everything on screen plus two screens ahead and half
    /// a screen behind, and recycle the rest.
    func updateVisible(fadeIn: Bool = false) {
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
                tile.frame = frame
                if fadeIn { tile.fadeIn() }
            }
            if !fadeIn {
                tile.frame = frame
                tile.layoutBadges()
            }
            if !tile.hasImage || tile.decodedPixels < ImagePipeline.bucket(pixels) {
                wanted.insert(ImagePipeline.key(timeline.id(at: index), pixels: pixels))
                if !speed.isFast { load(tile, index: index) }
            }
        }
        if !fadeIn {
            for (index, tile) in tiles where !keep.contains(index) {
                tile.isHidden = true
                tile.reset()
                tilePool.append(tile)
                tiles[index] = nil
            }
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
            if !fadeIn { header.opacity = 1 }
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
            tile.setImage(image, pixels: ImagePipeline.bucket(pixels))
        } else {
            tile.setPlaceholder(pipeline.placeholder(id, hash: timeline.thumbhash(index)))
        }
        tile.videoSeconds = timeline.isVideo(index) ? timeline.durationSeconds(index) : nil
        tile.isLive = timeline.isLive(index)
        tile.daysLeft = timeline.daysLeft(index)
        tile.isSelected = selection.contains(index)
    }

    private func load(_ tile: TileLayer, index: Int) {
        let pixels = Int(layout.side * (window?.backingScaleFactor ?? 2))
        pipeline.request(timeline.id(at: index), pixels: pixels) { [weak self, weak tile] image in
            guard let self, let tile, self.tiles[index] === tile else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            tile.setImage(image, pixels: ImagePipeline.bucket(pixels))
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
        effectiveAppearance.performAsCurrentDrawingAppearance { tile.applyColours(accent: accent) }
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

    // MARK: Clicks and the marquee

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        mouseDownPoint = point
        mouseDownIndex = layout.index(at: point)
        let flags = event.modifierFlags
        guard let index = mouseDownIndex else {
            // A month header selects the month; anything else starts a marquee.
            if let section = headerSection(at: point) {
                let month = IndexSet(integersIn: timeline.sections[section].start..<(timeline.sections[section].start + timeline.sections[section].count))
                select(flags.contains(.command) || flags.contains(.shift) ? selection.union(month) : month,
                       focus: timeline.sections[section].start, reveal: false)
                mouseDownPoint = nil
            } else {
                marqueeBase = flags.contains(.command) || flags.contains(.shift) ? selection : IndexSet()
                if marqueeBase?.isEmpty == true { select([], focus: nil, reveal: false) }
            }
            return
        }
        if event.clickCount == 2 {
            onOpen(index)
            return
        }
        if flags.contains(.shift), let anchor {
            let range = IndexSet(integersIn: min(anchor, index)...max(anchor, index))
            select(flags.contains(.command) ? selection.union(range) : range, focus: index, reveal: false)
        } else if flags.contains(.command) {
            var toggled = selection
            if toggled.contains(index) { toggled.remove(index) } else { toggled.insert(index) }
            anchor = index
            select(toggled, focus: index, reveal: false)
        } else if !selection.contains(index) {
            anchor = index
            select([index], focus: index, reveal: false)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = mouseDownPoint else { return }
        let point = convert(event.locationInWindow, from: nil)
        if mouseDownIndex != nil {
            if hypot(point.x - start.x, point.y - start.y) > 4 { beginDragging(with: event) }
            return
        }
        guard let base = marqueeBase else { return }
        autoscroll(with: event)
        let rect = CGRect(x: min(start.x, point.x), y: min(start.y, point.y),
                          width: abs(point.x - start.x), height: abs(point.y - start.y))
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        marquee.isHidden = false
        marquee.path = CGPath(rect: rect, transform: nil)
        CATransaction.commit()
        let inside = IndexSet(layout.elements(in: rect).tiles.map(\.index))
        select(base.union(inside), focus: inside.last ?? focus, reveal: false)
    }

    override func mouseUp(with event: NSEvent) {
        // A plain click on an already selected tile narrows the selection to it.
        if let index = mouseDownIndex, mouseDownPoint != nil, event.clickCount == 1,
           event.modifierFlags.intersection([.shift, .command]).isEmpty, selection.count > 1 {
            anchor = index
            select([index], focus: index, reveal: false)
        }
        marquee.isHidden = true
        marqueeBase = nil
        mouseDownPoint = nil
        mouseDownIndex = nil
    }

    private func headerSection(at point: CGPoint) -> Int? {
        guard !timeline.sections.isEmpty else { return nil }
        let s = layout.section(atOrBefore: point.y)
        return layout.headerFrame(s).contains(point) ? s : nil
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let point = convert(event.locationInWindow, from: nil)
        if let index = layout.index(at: point), !selection.contains(index) {
            anchor = index
            select([index], focus: index, reveal: false)
        }
        return selection.isEmpty ? nil : menuProvider()
    }

    // MARK: Keyboard

    override func keyDown(with event: NSEvent) {
        let extend = event.modifierFlags.contains(.shift)
        let current = focus ?? selection.last ?? -1
        let target: Int?
        switch event.specialKey {
        case .leftArrow?: target = max(0, current - 1)
        case .rightArrow?: target = min(timeline.count - 1, current + 1)
        case .upArrow?: target = max(0, current - columns)
        case .downArrow?: target = min(timeline.count - 1, current + columns)
        case .carriageReturn?, .enter?:
            if let focus { onOpen(focus) }
            return
        default:
            if event.characters == " ", let focus {
                onOpen(focus)
            } else {
                super.keyDown(with: event)
            }
            return
        }
        guard let target, timeline.count > 0 else { return }
        if extend, let anchor {
            select(IndexSet(integersIn: min(anchor, target)...max(anchor, target)), focus: target, reveal: true)
        } else {
            anchor = target
            select([target], focus: target, reveal: true)
        }
    }

    override func selectAll(_ sender: Any?) {
        guard timeline.count > 0 else { return }
        select(IndexSet(integersIn: 0..<timeline.count), focus: focus ?? 0, reveal: false)
    }

    override func cancelOperation(_ sender: Any?) {
        select([], focus: nil, reveal: false)
    }

    // MARK: Dragging out

    private func beginDragging(with event: NSEvent) {
        mouseDownPoint = nil
        guard let writer = promiseWriter, !selection.isEmpty else { return }
        // Large drags carry the first 500; Finder handles them one by one.
        let indexes = Array(selection.prefix(500))
        let ids = indexes.map { timeline.id(at: $0) }
        let names = filenames(ids)
        let items: [NSDraggingItem] = zip(indexes, zip(ids, names)).map { index, pair in
            let item = NSDraggingItem(pasteboardWriter: AssetFilePromise(assetID: pair.0, filename: pair.1.isEmpty ? "Photo" : pair.1, delegate: writer))
            let frame = self.frame(of: index)
            item.setDraggingFrame(frame, contents: tiles[index]?.contents)
            return item
        }
        let session = beginDraggingSession(with: items, event: event, source: self)
        session.draggingFormation = .stack
    }

    nonisolated func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        context == .outsideApplication ? .copy : [.copy, .generic]
    }

    // MARK: Zoom

    /// Pinch: the grid scales live under the fingers, then settles on the
    /// nearest column count when the fingers lift.
    override func magnify(with event: NSEvent) {
        switch event.phase {
        case .began:
            pinchScale = 1
            pinchAnchor = convert(event.locationInWindow, from: nil)
            pinchFrames = tiles.mapValues(\.frame)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            for header in headers.values { header.opacity = 0 }
            CATransaction.commit()
        case .changed:
            pinchScale = min(2.5, max(0.4, pinchScale * (1 + event.magnification)))
            let scale = CGAffineTransform(translationX: pinchAnchor.x, y: pinchAnchor.y)
                .scaledBy(x: pinchScale, y: pinchScale)
                .translatedBy(x: -pinchAnchor.x, y: -pinchAnchor.y)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            for (index, frame) in pinchFrames { tiles[index]?.frame = frame.applying(scale) }
            CATransaction.commit()
        case .ended, .cancelled:
            // The tiles are where the fingers left them; settle from there.
            let target = min(15, max(3, Int((CGFloat(columns) / pinchScale).rounded())))
            pinchScale = 1
            pinchFrames = [:]
            if target != columns {
                setColumns(target, anchor: pinchAnchor, animated: true)
                onColumnsChange(target)
            } else {
                setColumns(columns, anchor: nil, animated: true)
            }
        default:
            break
        }
    }
}

/// One tile: the image as the layer's contents, plus badges and a selection ring.
final class TileLayer: CALayer {
    private(set) var hasImage = false
    /// The size the shown image was decoded for, so zooming in can ask for a sharper one.
    private(set) var decodedPixels = 0
    private var badge: CATextLayer?
    private var liveBadge: CATextLayer?
    private var check: CALayer?

    /// A video's length, shown as a badge.
    var videoSeconds: Int? {
        didSet {
            guard videoSeconds != oldValue else { return }
            updateBadge()
        }
    }

    /// Days until purge, in Recently Deleted.
    var daysLeft: Int? {
        didSet {
            guard daysLeft != oldValue else { return }
            updateBadge()
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
        didSet {
            guard isSelected != oldValue else { return }
            borderWidth = isSelected ? 3 : 0
            if isSelected {
                let mark = check ?? makeCheck()
                check = mark
                mark.isHidden = false
                layoutBadges()
            } else {
                check?.isHidden = true
            }
        }
    }

    override init() {
        super.init()
        contentsGravity = .resizeAspectFill
        masksToBounds = true
    }

    override init(layer: Any) { super.init(layer: layer) }
    required init?(coder: NSCoder) { fatalError("not used") }

    func setImage(_ image: CGImage, pixels: Int) {
        contents = image
        hasImage = true
        decodedPixels = pixels
    }

    func setPlaceholder(_ image: CGImage?) {
        contents = image
        hasImage = false
        decodedPixels = 0
    }

    func reset() {
        contents = nil
        hasImage = false
        decodedPixels = 0
        videoSeconds = nil
        daysLeft = nil
        isLive = false
        isSelected = false
        opacity = 1
        removeAllAnimations()
    }

    func fadeIn() {
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = 0
        fade.toValue = 1
        fade.duration = 0.28
        add(fade, forKey: "fade")
    }

    func applyColours(accent: NSColor?) {
        let ring = accent ?? NSColor.controlAccentColor
        backgroundColor = NSColor.quaternaryLabelColor.cgColor
        borderColor = ring.cgColor
        check?.backgroundColor = ring.cgColor
    }

    func layoutBadges() {
        if let badge, !badge.isHidden {
            let width = CGFloat(badge.string.map { "\($0)".count } ?? 4) * 7 + 10
            badge.frame = CGRect(x: bounds.width - width - 4, y: bounds.height - 20, width: width, height: 16)
        }
        if let liveBadge, !liveBadge.isHidden {
            liveBadge.frame = CGRect(x: 4, y: 4, width: 34, height: 16)
        }
        if let check, !check.isHidden {
            check.frame = CGRect(x: bounds.width - 24, y: 4, width: 20, height: 20)
        }
    }

    private func updateBadge() {
        let text: String?
        if let daysLeft {
            text = daysLeft == 1 ? "1 day" : "\(daysLeft) days"
        } else if let seconds = videoSeconds {
            text = String(format: "%d:%02d", seconds / 60, seconds % 60)
        } else {
            text = nil
        }
        if let text {
            let label = badge ?? makeBadge()
            badge = label
            label.string = text
            label.isHidden = false
            layoutBadges()
        } else {
            badge?.isHidden = true
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

    private func makeCheck() -> CALayer {
        let mark = CATextLayer()
        mark.string = "✓"
        mark.font = NSFont.systemFont(ofSize: 13, weight: .bold)
        mark.fontSize = 13
        mark.alignmentMode = .center
        mark.foregroundColor = NSColor.white.cgColor
        mark.backgroundColor = borderColor ?? NSColor.controlAccentColor.cgColor
        mark.cornerRadius = 10
        mark.borderColor = NSColor.white.cgColor
        mark.borderWidth = 1.5
        mark.contentsScale = contentsScale
        addSublayer(mark)
        return mark
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

    func applyColours(background: NSColor?) {
        backgroundColor = (background ?? NSColor.windowBackgroundColor).withAlphaComponent(0.92).cgColor
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
