// Phase 0 check: open a library through the Rust core and list its albums.
// Usage: swift run ffi-smoke <data dir>. Point it at a copy, never at the
// live library.
import FramesCore
import Foundation

let arguments = CommandLine.arguments
guard arguments.count == 2 else {
    print("usage: ffi-smoke <data dir>")
    exit(2)
}

do {
    let library = try Library.open(dataDir: arguments[1])
    print("\(try library.assetCount()) items")
    for album in try library.albums() {
        print("album \"\(album.name)\": \(album.assetCount) items")
    }
    // While this one is open, a second open must be refused.
    do {
        _ = try Library.open(dataDir: arguments[1])
        print("lock: NOT enforced")
        exit(1)
    } catch FramesError.AlreadyOpen {
        print("lock: second open refused")
    }
} catch {
    print("error: \(error)")
    exit(1)
}
