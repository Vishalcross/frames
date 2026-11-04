import AppKit

/// The same grid as an NSCollectionView with a flow layout, sharing the image
/// pipeline and the fast-scroll rule, for the Phase 0 comparison.
final class TileItem: NSCollectionViewItem {
    var index = -1

    override func loadView() {
        view = NSView()
        view.wantsLayer = true
        view.layerContentsRedrawPolicy = .never
        view.layer?.contentsGravity = .resizeAspectFill
        view.layer?.masksToBounds = true
    }
}

final class MonthHeader: NSView, NSCollectionViewElement {
    let label = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        label.font = .boldSystemFont(ofSize: 15)
        label.frame = bounds.insetBy(dx: 12, dy: 12)
        label.autoresizingMask = [.width, .height]
        addSubview(label)
    }

    required init?(coder: NSCoder) { fatalError() }
}

final class CollectionGrid: NSObject, NSCollectionViewDataSource {
    let view = NSCollectionView()
    private let library: SyntheticLibrary
    private let pipeline: ImagePipeline
    private let speed = ScrollSpeed()
    private var pixel = 0
    private static let itemID = NSUserInterfaceItemIdentifier("tile")
    private static let headerID = NSUserInterfaceItemIdentifier("month")

    init(library: SyntheticLibrary, pipeline: ImagePipeline, width: CGFloat, columns: Int) {
        self.library = library
        self.pipeline = pipeline
        super.init()
        let grid = GridLayout(library: library, width: width, columns: columns)
        let flow = NSCollectionViewFlowLayout()
        flow.itemSize = NSSize(width: grid.side, height: grid.side)
        flow.minimumInteritemSpacing = grid.gap
        flow.minimumLineSpacing = grid.gap
        flow.sectionInset = NSEdgeInsets(top: 0, left: grid.gap, bottom: grid.gap, right: grid.gap)
        flow.headerReferenceSize = NSSize(width: width, height: grid.headerHeight)
        view.collectionViewLayout = flow
        view.register(TileItem.self, forItemWithIdentifier: Self.itemID)
        view.register(MonthHeader.self, forSupplementaryViewOfKind: NSCollectionView.elementKindSectionHeader,
                      withIdentifier: Self.headerID)
        view.dataSource = self
        view.isSelectable = false
        pixel = Int(grid.side * 2)
        speed.onSettle = { [weak self] in self?.fillVisible() }
    }

    func attach(to scrollView: NSScrollView) {
        scrollView.documentView = view
        let clip = scrollView.contentView
        clip.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(clipMoved), name: NSView.boundsDidChangeNotification, object: clip)
    }

    @objc private func clipMoved() {
        guard let visible = view.enclosingScrollView?.documentVisibleRect else { return }
        speed.update(y: visible.minY, viewport: visible.height)
        let visibleIndexes = Set(view.visibleItems().compactMap { ($0 as? TileItem)?.index })
        pipeline.cancel(notIn: visibleIndexes)
    }

    private func fillVisible() {
        for case let item as TileItem in view.visibleItems() where item.view.layer?.contents == nil {
            load(item)
        }
    }

    private func load(_ item: TileItem) {
        pipeline.request(item.index, pixelSize: pixel) { i, image in
            if item.index == i { item.view.layer?.contents = image }
        }
    }

    func numberOfSections(in collectionView: NSCollectionView) -> Int { library.sections.count }

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        library.sections[section].count
    }

    func collectionView(_ collectionView: NSCollectionView,
                        itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let item = collectionView.makeItem(withIdentifier: Self.itemID, for: indexPath) as! TileItem
        item.index = library.sections[indexPath.section].start + indexPath.item
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        item.view.layer?.backgroundColor = Placeholder.colour(item.index)
        item.view.layer?.contents = pipeline.cached(item.index)
        CATransaction.commit()
        if item.view.layer?.contents == nil, !speed.isFast { load(item) }
        return item
    }

    func collectionView(_ collectionView: NSCollectionView,
                        viewForSupplementaryElementOfKind kind: NSCollectionView.SupplementaryElementKind,
                        at indexPath: IndexPath) -> NSView {
        let header = collectionView.makeSupplementaryView(ofKind: kind, withIdentifier: Self.headerID, for: indexPath) as! MonthHeader
        header.label.stringValue = library.sections[indexPath.section].title
        return header
    }
}
