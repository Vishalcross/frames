// Bridge check: open a library through the Rust core and call what the Mac
// app uses, with timings. Usage: swift run -c release ffi-smoke <data dir>
// [<cache dir>]. Point it at copies, never at the live library.
import FramesCore
import Foundation

final class Quiet: LibraryObserver {
    func libraryChanged() {}
    func thumbReady(assetId: String) {}
    func syncState(online: Bool) {}
    func importChanged() {}
}

func timed<T>(_ label: String, _ work: () throws -> T) rethrows -> T {
    let start = Date()
    let result = try work()
    print(String(format: "%@: %.1f ms", label, Date().timeIntervalSince(start) * 1000))
    return result
}

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    print("usage: ffi-smoke <data dir> [<cache dir>]")
    exit(2)
}

do {
    let library = try timed("open") {
        try Library.open(dataDir: arguments[1], cacheDir: arguments.count > 2 ? arguments[2] : nil, observer: Quiet())
    }
    print("connection: \(library.connectionProblem() ?? "ok")")
    let timeline = try timed("timeline") { try library.timeline(filter: TimelineFilter(albumId: nil, kind: nil, trash: false, text: nil, year: nil, month: nil, camera: nil, fileExt: nil)) }
    print("\(timeline.count) items, \(timeline.ids.count) id bytes, \(timeline.thumbhashes.count) thumbhash bytes")
    for album in try library.albums() {
        print("album \"\(album.name)\": \(album.assetCount) items")
    }
    if timeline.count > 0 {
        let firstEnd = timeline.idEnds.withUnsafeBytes { $0.load(as: UInt32.self) }
        let first = String(decoding: timeline.ids.prefix(Int(firstEnd)), as: UTF8.self)
        if let detail = try library.assetDetail(assetId: first) {
            print("first: \(detail.filename), \(detail.width ?? 0)x\(detail.height ?? 0), camera \(detail.camera ?? "none"), albums \(detail.albums)")
        }
        print("thumb: \(timed("small thumb") { library.smallThumb(assetId: first) } ?? "none")")
        print("preview: \(library.preview(assetId: first) ?? "none")")
        print("original on this Mac: \(library.localOriginal(assetId: first) ?? "none")")
    }
    do {
        _ = try Library.open(dataDir: arguments[1], cacheDir: nil, observer: Quiet())
        print("lock: NOT enforced")
        exit(1)
    } catch FramesError.AlreadyOpen {
        print("lock: second open refused")
    }
} catch {
    print("error: \(error)")
    exit(1)
}
