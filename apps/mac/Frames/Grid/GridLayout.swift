import CoreGraphics

/// Square tiles under month headers. Everything is arithmetic on the section
/// table; finding what is on screen is a binary search, not a scan.
struct GridLayout {
    static let gap: CGFloat = 2
    static let headerHeight: CGFloat = 44

    let columns: Int
    let width: CGFloat
    let side: CGFloat
    let totalHeight: CGFloat
    private let sections: [TimelineSection]
    private let sectionTops: [CGFloat]

    init(sections: [TimelineSection], width: CGFloat, columns: Int) {
        self.columns = columns
        self.width = width
        self.sections = sections
        let gap = Self.gap
        side = max(1, ((width - gap * CGFloat(columns + 1)) / CGFloat(columns)).rounded(.down))
        var tops: [CGFloat] = []
        var y: CGFloat = 0
        for section in sections {
            tops.append(y)
            y += Self.headerHeight + CGFloat(Self.rows(in: section, columns: columns)) * (side + gap)
        }
        sectionTops = tops
        totalHeight = y
    }

    func headerFrame(_ section: Int) -> CGRect {
        CGRect(x: 0, y: sectionTops[section], width: width, height: Self.headerHeight)
    }

    func frame(of index: Int, inSection s: Int) -> CGRect {
        let offset = index - sections[s].start
        let row = offset / columns
        let column = offset % columns
        return CGRect(
            x: Self.gap + CGFloat(column) * (side + Self.gap),
            y: sectionTops[s] + Self.headerHeight + CGFloat(row) * (side + Self.gap),
            width: side, height: side)
    }

    /// Tiles and headers that meet `rect`, in order.
    func elements(in rect: CGRect) -> (tiles: [(index: Int, frame: CGRect)], headers: [(section: Int, frame: CGRect)]) {
        var tiles: [(Int, CGRect)] = []
        var headers: [(Int, CGRect)] = []
        guard !sections.isEmpty else { return ([], []) }
        var s = section(atOrBefore: rect.minY)
        while s < sections.count, sectionTops[s] < rect.maxY {
            let header = headerFrame(s)
            if header.intersects(rect) { headers.append((s, header)) }
            let gridTop = sectionTops[s] + Self.headerHeight
            let lastRowInSection = Self.rows(in: sections[s], columns: columns) - 1
            let firstRow = max(0, Int(((rect.minY - gridTop) / (side + Self.gap)).rounded(.down)))
            let lastRow = min(lastRowInSection, Int(((rect.maxY - gridTop) / (side + Self.gap)).rounded(.down)))
            if firstRow <= lastRow {
                for row in firstRow...lastRow {
                    for column in 0..<columns {
                        let offset = row * columns + column
                        guard offset < sections[s].count else { break }
                        tiles.append((sections[s].start + offset, frame(of: sections[s].start + offset, inSection: s)))
                    }
                }
            }
            s += 1
        }
        return (tiles, headers)
    }

    /// The tile under `point`, if any.
    func index(at point: CGPoint) -> Int? {
        guard !sections.isEmpty, point.x >= 0, point.y >= 0 else { return nil }
        let s = section(atOrBefore: point.y)
        let y = point.y - sectionTops[s] - Self.headerHeight
        guard y >= 0 else { return nil }
        let row = Int(y / (side + Self.gap))
        let column = Int((point.x - Self.gap) / (side + Self.gap))
        guard column >= 0, column < columns else { return nil }
        let offset = row * columns + column
        guard offset < sections[s].count else { return nil }
        let index = sections[s].start + offset
        return frame(of: index, inSection: s).contains(point) ? index : nil
    }

    func sectionTop(_ section: Int) -> CGFloat { sectionTops[section] }

    /// The section whose header is at or above `y`.
    func section(atOrBefore y: CGFloat) -> Int {
        var low = 0
        var high = sectionTops.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if sectionTops[mid] <= y { low = mid } else { high = mid - 1 }
        }
        return low
    }

    private static func rows(in section: TimelineSection, columns: Int) -> Int {
        (section.count + columns - 1) / columns
    }
}
