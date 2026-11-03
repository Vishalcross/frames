import AppKit
import ImageIO
import os

extension Notification.Name {
    /// A thumbnail that was missing is now on disk; `object` is the asset id.
    static let thumbnailArrived = Notification.Name("FramesThumbnailArrived")
}

/// Thumbnails for the grid and the Years and Months cards (frames_swift_spec.md
/// 7.4): a memory cache, then the file from the core (on disk, fetched from S4
/// or made from the original), decoded by ImageIO at the size it is shown at.
@MainActor
final class ImagePipeline {
    private let core: Core
    private let cache = NSCache<NSString, CGImage>()
    private let placeholders = NSCache<NSString, CGImage>()
    private let queue = OperationQueue()
    private var inFlight: [String: Request] = [:]
    /// Assets the core had no thumbnail for; tried again when one arrives.
    private var missing: Set<String> = []

    private struct Request {
        let cancellation: Cancellation
        var completions: [(CGImage) -> Void]
    }

    init(core: Core) {
        self.core = core
        cache.totalCostLimit = 150 * 1_048_576
        placeholders.countLimit = 5_000
        // Downloads from S4 can occupy a slot, so a few more than the 4 decoders.
        queue.maxConcurrentOperationCount = 6
        queue.qualityOfService = .userInitiated
    }

    /// Sizes are rounded up to a few steps, so zooming reuses earlier decodes.
    nonisolated static func bucket(_ pixels: Int) -> Int {
        for step in [128, 256, 384, 512, 768, 1024, 1600] where pixels <= step { return step }
        return 1600
    }

    nonisolated static func key(_ id: String, pixels: Int) -> String { "\(id)@\(bucket(pixels))" }

    func cached(_ id: String, pixels: Int) -> CGImage? {
        cache.object(forKey: Self.key(id, pixels: pixels) as NSString)
    }

    /// The blurred ThumbHash sketch to show until the thumbnail is decoded.
    func placeholder(_ id: String, hash: Data) -> CGImage? {
        if let image = placeholders.object(forKey: id as NSString) { return image }
        guard let image = ThumbHash.image(from: hash) else { return nil }
        placeholders.setObject(image, forKey: id as NSString)
        return image
    }

    /// Decode `id`'s thumbnail for `pixels` (the longer side on screen) and
    /// call `completion` with it on the main thread, unless cancelled first.
    func request(_ id: String, pixels: Int, completion: @escaping (CGImage) -> Void) {
        let key = Self.key(id, pixels: pixels)
        if let image = cache.object(forKey: key as NSString) {
            completion(image)
            return
        }
        guard !missing.contains(id) else { return }
        if inFlight[key] != nil {
            inFlight[key]?.completions.append(completion)
            return
        }
        let cancellation = Cancellation()
        inFlight[key] = Request(cancellation: cancellation, completions: [completion])
        let size = Self.bucket(pixels)
        let library = core.library
        queue.addOperation { [weak self] in
            guard !cancellation.isCancelled else { return }
            // Above 384 pixels the 300-pixel thumbnail would look soft (spec 7.4).
            let path = size > 384 ? library.preview(assetId: id) : library.smallThumb(assetId: id)
            let image = path.flatMap { Self.decode(URL(fileURLWithPath: $0), maxPixels: size) }
            let decoded = image.map(DecodedImage.init)
            Task { @MainActor in self?.finish(key: key, id: id, image: decoded, cancellation: cancellation) }
        }
    }

    /// Stop decoding everything except `keys` (see `key(_:pixels:)`).
    func cancel(keeping keys: Set<String>) {
        for (key, request) in inFlight where !keys.contains(key) {
            request.cancellation.cancel()
            inFlight[key] = nil
        }
    }

    /// The core produced a thumbnail that was missing.
    func thumbnailArrived(_ id: String) {
        guard missing.remove(id) != nil else { return }
        NotificationCenter.default.post(name: .thumbnailArrived, object: id)
    }

    private func finish(key: String, id: String, image: DecodedImage?, cancellation: Cancellation) {
        guard let request = inFlight[key], request.cancellation === cancellation else { return }
        inFlight[key] = nil
        guard let image = image?.cgImage else {
            if !cancellation.isCancelled { missing.insert(id) }
            return
        }
        cache.setObject(image, forKey: key as NSString, cost: image.bytesPerRow * image.height)
        for completion in request.completions { completion(image) }
    }

    /// Decode at no more than `maxPixels` on the longer side, oriented, and
    /// fully decoded here rather than later on the main thread.
    nonisolated static func decode(_ url: URL, maxPixels: Int) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }
}

/// A decoded image crossing from a decoder thread to the main thread. CGImage
/// is immutable, so sharing it is safe.
struct DecodedImage: @unchecked Sendable {
    let cgImage: CGImage
}

/// Set from the main thread, read by a decoder before it starts.
final class Cancellation: Sendable {
    private let state = OSAllocatedUnfairLock(initialState: false)
    var isCancelled: Bool { state.withLock { $0 } }
    func cancel() { state.withLock { $0 = true } }
}
