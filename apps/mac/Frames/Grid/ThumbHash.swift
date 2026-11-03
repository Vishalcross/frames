import CoreGraphics
import Foundation

/// ThumbHash (https://evanw.github.io/thumbhash, MIT licence), ported from the
/// reference JavaScript decoder: a ~25-byte sketch of a photo, drawn as a
/// tiny blurred image while the real thumbnail loads.
enum ThumbHash {
    static func image(from data: Data) -> CGImage? {
        let hash = [UInt8](data)
        guard hash.count >= 5 else { return nil }
        let header24 = Int(hash[0]) | Int(hash[1]) << 8 | Int(hash[2]) << 16
        let header16 = Int(hash[3]) | Int(hash[4]) << 8
        let lDC = Double(header24 & 63) / 63
        let pDC = Double((header24 >> 6) & 63) / 31.5 - 1
        let qDC = Double((header24 >> 12) & 63) / 31.5 - 1
        let lScale = Double((header24 >> 18) & 31) / 31
        let hasAlpha = header24 >> 23 != 0
        let pScale = Double((header16 >> 3) & 63) / 63
        let qScale = Double((header16 >> 9) & 63) / 63
        let isLandscape = header16 >> 15 != 0
        let lx = max(3, isLandscape ? (hasAlpha ? 5 : 7) : header16 & 7)
        let ly = max(3, isLandscape ? header16 & 7 : (hasAlpha ? 5 : 7))
        guard !hasAlpha || hash.count >= 6 else { return nil }
        let aDC = hasAlpha ? Double(hash[5] & 15) / 15 : 1
        let aScale = hasAlpha ? Double(hash[5] >> 4) / 15 : 0

        // The varying factors, with saturation boosted 1.25x for quantisation.
        let acStart = hasAlpha ? 6 : 5
        var acIndex = 0
        func channel(_ nx: Int, _ ny: Int, _ scale: Double) -> [Double]? {
            var ac: [Double] = []
            for cy in 0..<ny {
                var cx = cy == 0 ? 1 : 0
                while cx * ny < nx * (ny - cy) {
                    let byte = acStart + (acIndex >> 1)
                    guard byte < hash.count else { return nil }
                    let nibble = (Int(hash[byte]) >> ((acIndex & 1) << 2)) & 15
                    ac.append((Double(nibble) / 7.5 - 1) * scale)
                    acIndex += 1
                    cx += 1
                }
            }
            return ac
        }
        guard let lAC = channel(lx, ly, lScale),
              let pAC = channel(3, 3, pScale * 1.25),
              let qAC = channel(3, 3, qScale * 1.25) else { return nil }
        let aAC = hasAlpha ? channel(5, 5, aScale) : []
        guard let aAC else { return nil }

        let ratio = approximateAspectRatio(hash)
        let width = Int((ratio > 1 ? 32 : 32 * ratio).rounded())
        let height = Int((ratio > 1 ? 32 / ratio : 32).rounded())
        guard width > 0, height > 0 else { return nil }
        var rgba = [UInt8](repeating: 0, count: width * height * 4)
        var fx = [Double](repeating: 0, count: max(lx, hasAlpha ? 5 : 3))
        var fy = [Double](repeating: 0, count: max(ly, hasAlpha ? 5 : 3))
        var i = 0
        for y in 0..<height {
            for x in 0..<width {
                var l = lDC, p = pDC, q = qDC, a = aDC
                for cx in 0..<fx.count { fx[cx] = cos(.pi / Double(width) * (Double(x) + 0.5) * Double(cx)) }
                for cy in 0..<fy.count { fy[cy] = cos(.pi / Double(height) * (Double(y) + 0.5) * Double(cy)) }
                var j = 0
                for cy in 0..<ly {
                    var cx = cy == 0 ? 1 : 0
                    let fy2 = fy[cy] * 2
                    while cx * ly < lx * (ly - cy) {
                        l += lAC[j] * fx[cx] * fy2
                        j += 1
                        cx += 1
                    }
                }
                j = 0
                for cy in 0..<3 {
                    var cx = cy == 0 ? 1 : 0
                    let fy2 = fy[cy] * 2
                    while cx < 3 - cy {
                        let f = fx[cx] * fy2
                        p += pAC[j] * f
                        q += qAC[j] * f
                        j += 1
                        cx += 1
                    }
                }
                if hasAlpha {
                    j = 0
                    for cy in 0..<5 {
                        var cx = cy == 0 ? 1 : 0
                        let fy2 = fy[cy] * 2
                        while cx < 5 - cy {
                            a += aAC[j] * fx[cx] * fy2
                            j += 1
                            cx += 1
                        }
                    }
                }
                let b = l - 2 / 3 * p
                let r = (3 * l - b + q) / 2
                let g = r - q
                rgba[i] = UInt8(max(0, 255 * min(1, r)))
                rgba[i + 1] = UInt8(max(0, 255 * min(1, g)))
                rgba[i + 2] = UInt8(max(0, 255 * min(1, b)))
                rgba[i + 3] = UInt8(max(0, 255 * min(1, a)))
                i += 4
            }
        }
        guard let provider = CGDataProvider(data: Data(rgba) as CFData) else { return nil }
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: (hasAlpha ? CGImageAlphaInfo.last : .noneSkipLast).rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    private static func approximateAspectRatio(_ hash: [UInt8]) -> Double {
        let header = Int(hash[3])
        let hasAlpha = hash[2] & 0x80 != 0
        let isLandscape = hash[4] & 0x80 != 0
        let lx = isLandscape ? (hasAlpha ? 5 : 7) : header & 7
        let ly = isLandscape ? header & 7 : (hasAlpha ? 5 : 7)
        return ly == 0 ? 1 : Double(lx) / Double(ly)
    }
}
