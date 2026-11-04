import AppKit
import QuartzCore

/// Scrolls at a fixed speed, driven by the display link, and records how long
/// frames really took and how much main-thread work each scroll step cost.
final class Benchmark: NSObject {
    struct Result {
        let speed: Double
        let frames: Int
        let p50: Double, p95: Double, worst: Double
        let hitches: Int
        let hitchMsPerSecond: Double
        let mainP95: Double, mainWorst: Double
        let decodeP50: Double?, decodeP95: Double?
    }

    private let scrollView: NSScrollView
    private let pipeline: ImagePipeline
    private var link: CADisplayLink?
    private var speeds: [Double] = []
    private var seconds: Double = 5
    private var speed: Double = 0
    private var started: CFTimeInterval = 0
    private var last: CFTimeInterval = 0
    private var intervals: [Double] = []
    private var mainWork: [Double] = []
    private var frameBudget: Double = 1.0 / 60
    private var results: [Result] = []
    private var done: ([Result]) -> Void = { _ in }

    init(scrollView: NSScrollView, pipeline: ImagePipeline) {
        self.scrollView = scrollView
        self.pipeline = pipeline
    }

    func run(speeds: [Double], secondsEach: Double, done: @escaping ([Result]) -> Void) {
        self.speeds = speeds
        self.seconds = secondsEach
        self.done = done
        next()
    }

    private func next() {
        guard !speeds.isEmpty else {
            link?.invalidate()
            done(results)
            return
        }
        speed = speeds.removeFirst()
        intervals.removeAll()
        mainWork.removeAll()
        pipeline.resetStats()
        started = 0
        last = 0
        scroll(to: 0)
        // Let the first screen settle before measuring.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [self] in
            if link == nil {
                link = scrollView.displayLink(target: self, selector: #selector(tick(_:)))
                link?.add(to: .main, forMode: .common)
            }
            link?.isPaused = false
        }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        frameBudget = link.duration > 0 ? link.duration : frameBudget
        if started == 0 {
            started = now
            last = now
            return
        }
        let dt = now - last
        last = now
        intervals.append(dt)

        let viewport = scrollView.documentVisibleRect.height
        let y = scrollView.documentVisibleRect.minY + CGFloat(speed * dt) * viewport
        let t0 = CACurrentMediaTime()
        scroll(to: y)
        scrollView.layoutSubtreeIfNeeded()
        mainWork.append(CACurrentMediaTime() - t0)

        if now - started >= seconds {
            link.isPaused = true
            results.append(summarise())
            next()
        }
    }

    private func scroll(to y: CGFloat) {
        let clip = scrollView.contentView
        clip.setBoundsOrigin(NSPoint(x: 0, y: y))
        scrollView.reflectScrolledClipView(clip)
    }

    private func summarise() -> Result {
        let ms = intervals.map { $0 * 1000 }.sorted()
        let budget = frameBudget * 1000
        let late = ms.filter { $0 > budget * 1.5 }
        let total = intervals.reduce(0, +)
        let work = mainWork.map { $0 * 1000 }.sorted()
        let decodes = pipeline.decodeLatencies.sorted()
        return Result(
            speed: speed, frames: ms.count,
            p50: percentile(ms, 0.5), p95: percentile(ms, 0.95), worst: ms.last ?? 0,
            hitches: late.count,
            hitchMsPerSecond: late.map { $0 - budget }.reduce(0, +) / max(total, 0.001),
            mainP95: percentile(work, 0.95), mainWorst: work.last ?? 0,
            decodeP50: decodes.isEmpty ? nil : percentile(decodes, 0.5),
            decodeP95: decodes.isEmpty ? nil : percentile(decodes, 0.95))
    }

    private func percentile(_ sorted: [Double], _ p: Double) -> Double {
        guard !sorted.isEmpty else { return 0 }
        return sorted[min(sorted.count - 1, Int(Double(sorted.count - 1) * p))]
    }
}

func memoryFootprintMB() -> Double {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let status = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return status == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
}
