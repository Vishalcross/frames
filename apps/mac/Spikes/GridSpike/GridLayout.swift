import CoreGraphics

/// Square tiles in month sections. Everything is arithmetic on the section
/// table; finding what is on screen is a binary search, not a scan.
struct GridLayout {
    let columns: Int
    let gap: CGFloat = 2
    let headerHeight: CGFloat = 44
    let side: CGFloat
    let width: CGFloat
    let sectionTops: [CGFloat]
    let totalHeight: CGFloat
    private let sections: [Section]

    init(library: SyntheticLibrary, width: CGFloat, columns: Int) {
        self.columns = columns
        self.width = width
        self.sections = library.sections
        side = ((width - gap * CGFloat(columns + 1)) / CGFloat(columns)).rounded(.down)
        var tops: [CGFloat] = []
        var y: CGFloat = 0
        for section in library.sections {
            tops.append(y)
            let rows = (section.count + columns - 1) / columns
            y += headerHeight + CGFloat(rows) * (side + gap)
        }
        sectionTops = tops
        totalHeight = y
    }

    func headerFrame(_ s: Int) -> CGRect {
        CGRect(x: 0, y: sectionTops[s], width: width, height: headerHeight)
    }

    /// Items and headers whose frames meet `rect`, in order.
    func elements(in rect: CGRect) -> (items: [(Int, CGRect)], headers: [(Int, CGRect)]) {
        var items: [(Int, CGRect)] = []
        var headers: [(Int, CGRect)] = []
        var s = firstSection(atOrBefore: rect.minY)
        while s < sections.count, sectionTops[s] < rect.maxY {
            let header = headerFrame(s)
            if header.intersects(rect) { headers.append((s, header)) }
            let section = sections[s]
            let gridTop = sectionTops[s] + headerHeight
            let rows = (section.count + columns - 1) / columns
            let firstRow = max(0, Int(((rect.minY - gridTop) / (side + gap)).rounded(.down)))
            let lastRow = min(rows - 1, Int(((rect.maxY - gridTop) / (side + gap)).rounded(.down)))
            if firstRow <= lastRow {
                for row in firstRow...lastRow {
                    for column in 0..<columns {
                        let offset = row * columns + column
                        if offset >= section.count { break }
                        let frame = CGRect(
                            x: gap + CGFloat(column) * (side + gap),
                            y: gridTop + CGFloat(row) * (side + gap),
                            width: side, height: side)
                        items.append((section.start + offset, frame))
                    }
                }
            }
            s += 1
        }
        return (items, headers)
    }

    private func firstSection(atOrBefore y: CGFloat) -> Int {
        var low = 0
        var high = sectionTops.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if sectionTops[mid] <= y { low = mid } else { high = mid - 1 }
        }
        return low
    }
}
