// Phase 0 video trial (frames_swift_spec.md section 10): does AVPlayer play
// each URL, how soon is it ready, and does playback advance?
// Usage: video-spike <url> [<url> ...]
import AVFoundation
import Foundation

func seconds(_ value: Double) -> String { String(format: "%.1f s", value) }

func probe(_ url: URL) async -> String {
    let item = AVPlayerItem(url: url)
    let player = AVPlayer(playerItem: item)
    player.isMuted = true
    let start = Date()
    while item.status == .unknown, Date().timeIntervalSince(start) < 15 {
        try? await Task.sleep(for: .milliseconds(50))
    }
    let readyAfter = Date().timeIntervalSince(start)
    guard item.status == .readyToPlay else {
        let error = item.error as NSError?
        let reason = error.map { "\($0.localizedDescription) [\($0.domain) \($0.code)]" } ?? "no answer"
        let log = item.errorLog()?.events.last.map { "; log \($0.errorStatusCode) \($0.errorComment ?? "")" } ?? ""
        return "FAIL after \(seconds(readyAfter)): \(reason)\(log)"
    }
    player.play()
    try? await Task.sleep(for: .seconds(3))
    let reached = player.currentTime().seconds
    let duration = item.duration.seconds
    player.pause()
    return "OK ready in \(seconds(readyAfter)); played to \(seconds(reached)) after 3 s; duration "
        + (duration.isFinite ? seconds(duration) : "unknown")
}

for argument in CommandLine.arguments.dropFirst() {
    guard let url = URL(string: argument) else { continue }
    print("\(url.lastPathComponent.prefix(40)): \(await probe(url))")
}
