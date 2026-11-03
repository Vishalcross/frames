import Foundation

/// A month of the timeline: its header and the assets under it.
struct TimelineSection: Sendable, Equatable {
    let start: Int
    let count: Int
    let year: Int
    let month: Int

    var title: String { "\(Calendar.current.standaloneMonthSymbols[month - 1]) \(year)" }
}

/// A whole timeline held in memory (frames_swift_spec.md 5.3): the core's
/// packed arrays, read by index, newest first. About 50 bytes per asset.
struct Timeline: Sendable {
    static let empty = Timeline()

    /// Changes whenever the contents may have, so views can tell cheaply.
    let version: UUID
    let count: Int
    let sections: [TimelineSection]
    private let ids: Data
    private let idEnds: [UInt32]
    private let localMs: [Int64]
    private let flags: Data
    private let durations: [UInt32]
    private let thumbhashes: Data
    private let thumbhashEnds: [UInt32]

    private init() {
        version = UUID()
        count = 0
        sections = []
        ids = Data()
        idEnds = []
        localMs = []
        flags = Data()
        durations = []
        thumbhashes = Data()
        thumbhashEnds = []
    }

    init(_ snapshot: TimelineSnapshot) {
        version = UUID()
        count = Int(snapshot.count)
        ids = snapshot.ids
        idEnds = Self.integers(snapshot.idEnds)
        localMs = Self.integers(snapshot.localMs)
        flags = snapshot.flags
        durations = Self.integers(snapshot.durationS)
        thumbhashes = snapshot.thumbhashes
        thumbhashEnds = Self.integers(snapshot.thumbhashEnds)
        sections = Self.sections(localMs)
    }

    /// The core's little-endian numbers as an array, in one copy. The app is
    /// arm64 only, which is little-endian, so no byte swapping is needed.
    private static func integers<T: FixedWidthInteger>(_ data: Data) -> [T] {
        let count = data.count / MemoryLayout<T>.stride
        return [T](unsafeUninitializedCapacity: count) { buffer, initialized in
            _ = data.copyBytes(to: buffer)
            initialized = count
        }
    }

    func id(at index: Int) -> String {
        String(decoding: ids[range(index, ends: idEnds)], as: UTF8.self)
    }

    /// When the photo was taken, as wall-clock time read in UTC.
    func date(at index: Int) -> Date {
        Date(timeIntervalSince1970: Double(localMs[index]) / 1000)
    }

    func isVideo(_ index: Int) -> Bool { flags[index] & 1 != 0 }
    func isLive(_ index: Int) -> Bool { flags[index] & 2 != 0 }
    func dateGuessed(_ index: Int) -> Bool { flags[index] & 4 != 0 }
    func durationSeconds(_ index: Int) -> Int { Int(durations[index]) }
    func thumbhash(_ index: Int) -> Data { thumbhashes[range(index, ends: thumbhashEnds)] }

    func index(of id: String) -> Int? {
        let wanted = Array(id.utf8)
        return (0..<count).first { ids[range($0, ends: idEnds)].elementsEqual(wanted) }
    }

    func section(containing index: Int) -> Int? {
        var low = 0
        var high = sections.count - 1
        while low <= high {
            let mid = (low + high) / 2
            let section = sections[mid]
            if index < section.start {
                high = mid - 1
            } else if index >= section.start + section.count {
                low = mid + 1
            } else {
                return mid
            }
        }
        return nil
    }

    private func range(_ index: Int, ends: [UInt32]) -> Range<Int> {
        (index == 0 ? 0 : Int(ends[index - 1]))..<Int(ends[index])
    }

    /// Month boundaries, newest first. Calendar fields are only computed when
    /// a date leaves the current month, so this is one pass over the dates.
    private static func sections(_ localMs: [Int64]) -> [TimelineSection] {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        var sections: [TimelineSection] = []
        var start = 0
        var month: (year: Int, month: Int, from: Int64, until: Int64)?
        for (index, ms) in localMs.enumerated() {
            if let current = month, ms >= current.from, ms < current.until { continue }
            if let current = month {
                sections.append(TimelineSection(start: start, count: index - start, year: current.year, month: current.month))
                start = index
            }
            let date = Date(timeIntervalSince1970: Double(ms) / 1000)
            let interval = calendar.dateInterval(of: .month, for: date)!
            let fields = calendar.dateComponents([.year, .month], from: date)
            month = (fields.year!, fields.month!,
                     Int64(interval.start.timeIntervalSince1970 * 1000), Int64(interval.end.timeIntervalSince1970 * 1000))
        }
        if let current = month {
            sections.append(TimelineSection(start: start, count: localMs.count - start, year: current.year, month: current.month))
        }
        return sections
    }
}
