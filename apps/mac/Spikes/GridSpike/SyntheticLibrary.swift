import Foundation

/// A month of items: the grid draws a header, then the month's tiles.
struct Section {
    let start: Int
    let count: Int
    let title: String
}

/// `count` items in months of 20 to 400, newest first. Item `i` shows
/// `files[i % files.count]`, so every item still needs its own decode.
struct SyntheticLibrary {
    let count: Int
    let sections: [Section]
    let files: [URL]

    init(count: Int, files: [URL]) {
        var random = SplitMix64(seed: 42)
        var sections: [Section] = []
        var start = 0
        var monthsBack = 0
        let formatter = DateFormatter()
        formatter.dateFormat = "LLLL yyyy"
        while start < count {
            let size = min(count - start, 20 + Int(random.next() % 381))
            let date = Calendar.current.date(byAdding: .month, value: -monthsBack, to: Date())!
            sections.append(Section(start: start, count: size, title: formatter.string(from: date)))
            start += size
            monthsBack += 1
        }
        self.count = count
        self.sections = sections
        self.files = files
    }

    func file(for index: Int) -> URL { files[index % files.count] }

    /// The cached small thumbnails of the owner's library, read only.
    static func cachedThumbnails() -> [URL] {
        let root = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Caches/app.frames.desktop/cache/thumbs_s")
        let walker = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil)
        var files: [URL] = []
        while let url = walker?.nextObject() as? URL {
            if url.pathExtension == "webp" { files.append(url) }
        }
        return files.sorted { $0.path < $1.path }
    }
}

struct SplitMix64 {
    private var state: UInt64
    init(seed: UInt64) { state = seed }
    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
}
