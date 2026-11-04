import AppKit
import ImageIO

/// Memory cache, then an ImageIO decode at the tile's pixel size on a
/// background queue. Main-thread API; completions arrive on the main thread.
final class ImagePipeline {
    private let library: SyntheticLibrary
    private let cache = NSCache<NSNumber, CGImage>()
    private let queue = OperationQueue()
    private var inFlight: [Int: Operation] = [:]
    private(set) var decodeLatencies: [Double] = []

    init(library: SyntheticLibrary, costLimitMB: Int) {
        self.library = library
        cache.totalCostLimit = costLimitMB * 1_048_576
        // One decode per performance core (4 on the M3).
        queue.maxConcurrentOperationCount = 4
        queue.qualityOfService = .userInitiated
    }

    func cached(_ index: Int) -> CGImage? { cache.object(forKey: NSNumber(value: index)) }

    func request(_ index: Int, pixelSize: Int, completion: @escaping (Int, CGImage) -> Void) {
        if inFlight[index] != nil { return }
        let url = library.file(for: index)
        let asked = CACurrentMediaTime()
        let operation = BlockOperation()
        operation.addExecutionBlock { [weak operation] in
            guard let operation, !operation.isCancelled else { return }
            let image = Self.decode(url, maxPixel: pixelSize)
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.inFlight[index] = nil
                guard let image, !operation.isCancelled else { return }
                self.cache.setObject(image, forKey: NSNumber(value: index), cost: image.bytesPerRow * image.height)
                self.decodeLatencies.append((CACurrentMediaTime() - asked) * 1000)
                completion(index, image)
            }
        }
        inFlight[index] = operation
        queue.addOperation(operation)
    }

    /// Cancel decodes for items that left the prefetch area.
    func cancel(notIn keep: Set<Int>) {
        for (index, operation) in inFlight where !keep.contains(index) {
            operation.cancel()
            inFlight[index] = nil
        }
    }

    func resetStats() { decodeLatencies.removeAll() }

    private static func decode(_ url: URL, maxPixel: Int) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }
}

/// Muted placeholder colours, standing in for ThumbHash.
enum Placeholder {
    static let colours: [CGColor] = (0..<32).map { i in
        NSColor(hue: CGFloat(i) / 32, saturation: 0.25, brightness: 0.45, alpha: 1).cgColor
    }
    static func colour(_ index: Int) -> CGColor { colours[index % colours.count] }
}

/// Scroll speed in screens per second. Above 3, tiles show placeholders and
/// decoding waits until the scroll settles (frames_swift_spec.md 7.4).
final class ScrollSpeed {
    private(set) var isFast = false
    var onSettle: () -> Void = {}
    private var lastY: CGFloat = 0
    private var lastTime: CFTimeInterval = 0
    private var speed: Double = 0
    private var settleTimer: Timer?

    func update(y: CGFloat, viewport: CGFloat) {
        let now = CACurrentMediaTime()
        let dt = now - lastTime
        if lastTime > 0, dt > 0, dt < 0.25, viewport > 0 {
            let instant = Double(abs(y - lastY)) / Double(viewport) / dt
            speed = speed * 0.7 + instant * 0.3
        }
        lastY = y
        lastTime = now
        isFast = speed > 3
        settleTimer?.invalidate()
        settleTimer = Timer.scheduledTimer(withTimeInterval: 0.12, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.speed = 0
            self.isFast = false
            self.onSettle()
        }
    }
}
