import SwiftUI

/// The grid in a scroll view, for SwiftUI.
struct TimelineGrid: NSViewRepresentable {
    let timeline: Timeline
    let pipeline: ImagePipeline
    @Binding var columns: Int
    @Binding var selection: Int?
    /// Set to scroll a tile to the top; the id makes repeated requests distinct.
    var scrollTarget: ScrollTarget?
    var onOpen: (Int) -> Void

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = false
        let grid = GridView(pipeline: pipeline)
        scrollView.documentView = grid
        grid.onSelect = { index in
            DispatchQueue.main.async { if selection != index { selection = index } }
        }
        grid.onColumnsChange = { count in
            DispatchQueue.main.async { columns = count }
        }
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let grid = scrollView.documentView as? GridView else { return }
        grid.onOpen = onOpen
        grid.show(timeline, columns: columns)
        if grid.columns != columns { grid.setColumns(columns, anchor: nil) }
        if grid.selection != selection { grid.select(selection) }
        if let scrollTarget, context.coordinator.lastScroll != scrollTarget.id {
            context.coordinator.lastScroll = scrollTarget.id
            grid.scroll(toTop: scrollTarget.index)
        }
        if scrollView.window?.firstResponder === scrollView.window { scrollView.window?.makeFirstResponder(grid) }
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
