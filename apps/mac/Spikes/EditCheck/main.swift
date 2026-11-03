// Edit check: every edit the Mac app makes, through the bridge, on a copy
// of a library. Usage: swift run edit-check <data dir> <cache dir> <export dir>.
// Never point it at the live library: it changes what it opens.
import FramesCore
import Foundation

final class Quiet: LibraryObserver {
    func libraryChanged() {}
    func thumbReady(assetId: String) {}
    func syncState(online: Bool) {}
    func importChanged() {}
}

func check(_ condition: Bool, _ label: String) {
    print("\(condition ? "ok  " : "FAIL") \(label)")
    if !condition { exit(1) }
}

let arguments = CommandLine.arguments
guard arguments.count == 4 else {
    print("usage: edit-check <data dir> <cache dir> <export dir>")
    exit(2)
}
let library = try Library.open(dataDir: arguments[1], cacheDir: arguments[2], observer: Quiet())
let all = TimelineFilter(albumId: nil, kind: nil, trash: false, text: nil, year: nil, month: nil, camera: nil, fileExt: nil)

func ids(_ filter: TimelineFilter) throws -> [String] {
    let t = try library.timeline(filter: filter)
    var result: [String] = []
    var start = 0
    t.idEnds.withUnsafeBytes { ends in
        for i in 0..<Int(t.count) {
            let end = Int(ends.load(fromByteOffset: i * 4, as: UInt32.self))
            result.append(String(decoding: t.ids[start..<end], as: UTF8.self))
            start = end
        }
    }
    return result
}

let before = try ids(all)
let three = Array(before.prefix(3))
let albumsBefore = try library.albums().count

let albumID = try library.createAlbum(name: "Edit Check", assetIds: three)
var album = try library.albums().first { $0.id == albumID }
check(album?.assetCount == 3 && album?.name == "Edit Check", "create album with 3 photos")
try library.renameAlbum(albumId: albumID, name: "Edit Check Renamed")
check(try library.albums().first { $0.id == albumID }?.name == "Edit Check Renamed", "rename album")
var inAlbum = all
inAlbum.albumId = albumID
check(try ids(inAlbum).count == 3, "album timeline holds 3")

let otherID = try library.createAlbum(name: "Edit Check Other", assetIds: [])
try library.moveToAlbum(fromAlbumId: albumID, toAlbumId: otherID, assetIds: [three[0]])
album = try library.albums().first { $0.id == albumID }
let other = try library.albums().first { $0.id == otherID }
check(album?.assetCount == 2 && other?.assetCount == 1, "move one to another album")
try library.removeFromAlbum(albumId: albumID, assetIds: [three[1]])
check(try library.albums().first { $0.id == albumID }?.assetCount == 1, "remove from album")
try library.addToAlbum(albumId: albumID, assetIds: [three[1]])
check(try library.albums().first { $0.id == albumID }?.assetCount == 2, "add to album")

try library.trash(assetIds: [three[2]])
check(try !ids(all).contains(three[2]), "trashed photo leaves the library")
var trash = all
trash.trash = true
let trashed = try library.timeline(filter: trash)
let trashIDs = try ids(trash)
let days = trashed.daysLeft.withUnsafeBytes { $0.load(as: UInt16.self) }
check(trashIDs.contains(three[2]) && days >= 29, "Recently Deleted holds it with \(days) days left")
check(try library.trashSummary().count >= 1, "trash summary counts it")
try library.restore(assetIds: [three[2]])
check(try ids(all).contains(three[2]), "restore brings it back")

let detail = try library.assetDetail(assetId: three[0])!
let newTaken = detail.takenUtcMs - 86_400_000 * 365
try library.setDate(assetId: three[0], takenUtcMs: newTaken)
check(try library.assetDetail(assetId: three[0])?.takenUtcMs == newTaken, "adjust date")
try library.setDate(assetId: three[0], takenUtcMs: detail.takenUtcMs)

let report = try library.exportOriginals(assetIds: [three[0], three[1]], folder: arguments[3])
let exported = try FileManager.default.contentsOfDirectory(atPath: arguments[3])
check(report.exported == 2 && exported.count >= 2, "export 2 originals from this Mac: \(exported.sorted())")
let single = arguments[3] + "/dragged-" + detail.filename
try library.exportOriginal(assetId: three[0], path: single)
check(FileManager.default.fileExists(atPath: single), "export one original to an exact path (drag to Finder)")

let year = Int32(Calendar(identifier: .gregorian).component(.year, from: Date(timeIntervalSince1970: Double(detail.takenUtcMs) / 1000)))
let suggestions = try library.searchSuggestions(text: String(year))
check(!suggestions.dates.isEmpty, "search suggests dates for \(year): \(suggestions.dates)")
var search = all
search.text = (detail.filename as NSString).deletingPathExtension
check(try ids(search).contains(three[0]), "text search finds \(detail.filename)")
check(try library.filenames(assetIds: three).first == detail.filename, "filenames for dragging")

try library.deleteAlbum(albumId: albumID)
try library.deleteAlbum(albumId: otherID)
let albumsAfter = try library.albums().count
let afterCount = try ids(all).count
check(albumsAfter == albumsBefore && afterCount == before.count, "delete albums, photos stay")
do {
    _ = try library.emptyTrash()
    check(false, "empty trash needs S4")
} catch FramesError.NotConnected {
    check(true, "empty trash refuses without S4")
}
print("all edits work")
