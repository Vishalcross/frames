import SwiftUI

/// The grid in a scroll view, with the year scrubber, for SwiftUI.
struct TimelineGrid: NSViewRepresentable {
    @Environment(AppModel.self) private var model
    let timeline: Timeline
    let pipeline: ImagePipeline
    var menu: () -> NSMenu?

    func makeNSView(context: Context) -> GridContainer {
        let container = GridContainer(pipeline: pipeline)
        let grid = container.grid
        let model = self.model
        grid.onSelectionChange = { selection, focus in
            model.selection = selection
            model.focus = focus
        }
        grid.onColumnsChange = { count in model.columns = count }
        grid.onOpen = { index in model.openViewer(at: index) }
        if let core = model.core {
            grid.promiseWriter = AssetPromiseWriter(core: core)
            grid.filenames = { ids in (try? core.library.filenames(assetIds: ids)) ?? [] }
        }
        model.transitions.grid = grid
        return container
    }

    func updateNSView(_ container: GridContainer, context: Context) {
        let grid = container.grid
        grid.menuProvider = menu
        grid.show(timeline, columns: model.columns)
        grid.setSelection(model.selection, focus: model.focus)
        container.refreshScrubber()
        if let target = model.scrollTarget, context.coordinator.lastScroll != target.id {
            context.coordinator.lastScroll = target.id
            grid.scroll(toTop: target.index)
        }
        if let window = container.window, window.firstResponder === window || window.firstResponder == nil {
            window.makeFirstResponder(grid)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        var lastScroll: UUID?
    }
}

struct ScrollTarget: Equatable {
    let index: Int
    let id = UUID()
}

/// The scroll view and the year scrubber beside it.
final class GridContainer: NSView {
    let scrollView = NSScrollView()
    let grid: GridView
    private let scrubber = YearScrubber()

    init(pipeline: ImagePipeline) {
        grid = GridView(pipeline: pipeline)
        super.init(frame: .zero)
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = false
        scrollView.documentView = grid
        addSubview(scrollView)
        addSubview(scrubber)
        scrubber.onScrub = { [weak self] fraction in self?.grid.scroll(toFraction: fraction) }
        scrubber.titleAt = { [weak self] fraction in self?.grid.monthTitle(atFraction: fraction) ?? "" }
        grid.onScroll = { [weak self] in
            guard let self, !self.scrubber.isHidden else { return }
            self.scrubber.position = self.grid.scrollFraction
            self.scrubber.flash()
        }
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func layout() {
        super.layout()
        scrollView.frame = bounds
        scrubber.frame = CGRect(x: bounds.width - 64, y: 0, width: 50, height: bounds.height)
        refreshScrubber()
    }

    /// Years only help once there is more than a few screens to move through.
    func refreshScrubber() {
        let tall = grid.frame.height > scrollView.contentView.bounds.height * 3
        scrubber.isHidden = !tall
        if tall {
            scrubber.marks = grid.yearMarks
            scrubber.position = grid.scrollFraction
        }
    }
}
