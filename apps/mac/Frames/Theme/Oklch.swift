import AppKit

/// Colour maths for the Adaptive appearance, ported from the Tauri app's
/// `frames-shell/src/lib/theme/palette.ts` so both apps make the same colours.
struct Oklch {
    var l: Double
    var c: Double
    var h: Double

    init(l: Double, c: Double, h: Double) {
        self.l = l
        self.c = c
        self.h = h
    }

    /// From sRGB components in 0...1.
    init(r: Double, g: Double, b: Double) {
        func linear(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
        let (R, G, B) = (linear(r), linear(g), linear(b))
        let l = cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B)
        let m = cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B)
        let s = cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B)
        let L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
        let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
        let bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
        var hue = atan2(bb, a) * 180 / .pi
        if hue < 0 { hue += 360 }
        self.init(l: L, c: (a * a + bb * bb).squareRoot(), h: hue)
    }

    /// Linear sRGB, unclamped, so callers can tell when it is out of gamut.
    var linearRGB: (Double, Double, Double) {
        let radians = h * .pi / 180
        let a = c * cos(radians), b = c * sin(radians)
        let l_ = l + 0.3963377774 * a + 0.2158037573 * b
        let m_ = l - 0.1055613458 * a - 0.0638541728 * b
        let s_ = l - 0.0894841775 * a - 1.291485548 * b
        let (L, M, S) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_)
        return (4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S,
                -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S,
                -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S)
    }

    /// This colour with its chroma reduced until it fits in sRGB.
    var fitted: Oklch {
        var colour = self
        func inside(_ v: (Double, Double, Double)) -> Bool {
            [v.0, v.1, v.2].allSatisfy { $0 >= -0.001 && $0 <= 1.001 }
        }
        while colour.c > 0 && !inside(colour.linearRGB) { colour.c = max(0, colour.c - 0.004) }
        return colour
    }

    var nsColor: NSColor {
        let (r, g, b) = fitted.linearRGB
        func encode(_ v: Double) -> Double {
            let x = min(1, max(0, v))
            return x <= 0.0031308 ? 12.92 * x : 1.055 * pow(x, 1 / 2.4) - 0.055
        }
        return NSColor(srgbRed: encode(r), green: encode(g), blue: encode(b), alpha: 1)
    }

    /// WCAG relative luminance, for contrast checks.
    var luminance: Double {
        let (r, g, b) = fitted.linearRGB
        return 0.2126 * min(1, max(0, r)) + 0.7152 * min(1, max(0, g)) + 0.0722 * min(1, max(0, b))
    }
}

extension NSColor {
    /// `#RRGGBB`, or nil.
    convenience init?(hex: String) {
        let digits = hex.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        self.init(srgbRed: Double((value >> 16) & 255) / 255, green: Double((value >> 8) & 255) / 255,
                  blue: Double(value & 255) / 255, alpha: 1)
    }

    var oklch: Oklch {
        let c = usingColorSpace(.sRGB) ?? self
        return Oklch(r: c.redComponent, g: c.greenComponent, b: c.blueComponent)
    }

    /// HSL saturation, 0...1.
    var hslSaturation: Double {
        let c = usingColorSpace(.sRGB) ?? self
        let high = max(c.redComponent, c.greenComponent, c.blueComponent)
        let low = min(c.redComponent, c.greenComponent, c.blueComponent)
        let lightness = (high + low) / 2
        return high == low ? 0 : (high - low) / (1 - abs(2 * lightness - 1))
    }
}
