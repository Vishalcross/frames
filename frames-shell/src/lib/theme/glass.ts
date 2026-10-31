/**
 * Liquid Glass Runtime & Capabilities (§8)
 * - Tier 1 vs Tier 2 SVG displacement detection
 * - Pointer specular rAF tracking
 * - Adaptive foreground luminance flip (0.62 / 0.50 hysteresis)
 * - Performance Governor (rAF frame timer, stepping down L0..L3)
 */

export type GlassGovernorLevel = 'L0' | 'L1' | 'L2' | 'L3';

let detectedTier: 1 | 2 = 1;
let governorLevel: GlassGovernorLevel = 'L1';
let isGovernorRunning = false;
let frameTimes: number[] = [];
let lastFrameTimestamp = 0;
let poorPerformanceDuration = 0;
let idleDuration = 0;

/**
 * Detect Tier 2 SVG backdrop-filter capability (§8.3)
 */
export function detectGlassTier(): 1 | 2 {
  if (typeof window === 'undefined' || typeof CSS === 'undefined') return 1;
  try {
    const supportsSvgBackdrop =
      CSS.supports('-webkit-backdrop-filter', 'url(#test)') ||
      CSS.supports('backdrop-filter', 'url(#test)');

    // In WKWebView (macOS) SVG inside backdrop-filter is typically not supported yet,
    // Chromium / WebView2 supports it.
    if (supportsSvgBackdrop) {
      detectedTier = 2;
    } else {
      detectedTier = 1;
    }
  } catch {
    detectedTier = 1;
  }
  return detectedTier;
}

/**

 * Adaptive foreground color calculation based on underlying tile luminance (§8.4)
 * Flip to dark above 0.62, back to light below 0.50 (hysteresis)
 */
let currentGlassFg: 'light' | 'dark' = 'light';

export function resolveGlassForeground(lum: number | null): 'light' | 'dark' {
  if (lum === null || lum === undefined) return currentGlassFg;
  if (currentGlassFg === 'light' && lum > 0.62) {
    currentGlassFg = 'dark';
  } else if (currentGlassFg === 'dark' && lum < 0.50) {
    currentGlassFg = 'light';
  }
  return currentGlassFg;
}

/**
 * Performance Governor (§8.5)
 * Monitors frame time via rAF during scroll/animations.
 * If p95 frame time > 18ms for 2s, step down: L0 -> L1 -> L2 -> L3.
 */
export function startPerformanceGovernor(onLevelChange?: (level: GlassGovernorLevel) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  if (isGovernorRunning) return () => {};
  isGovernorRunning = true;
  frameTimes = [];
  lastFrameTimestamp = performance.now();
  poorPerformanceDuration = 0;
  idleDuration = 0;

  let animId: number;

  const loop = (now: number) => {
    const delta = now - lastFrameTimestamp;
    lastFrameTimestamp = now;

    if (delta > 0 && delta < 200) {
      frameTimes.push(delta);
      if (frameTimes.length > 60) frameTimes.shift();

      // Check p95 frame time every ~30 frames
      if (frameTimes.length >= 30) {
        const sorted = [...frameTimes].sort((a, b) => a - b);
        const p95 = sorted[Math.floor(sorted.length * 0.95)];

        if (p95 > 18.0) {
          poorPerformanceDuration += delta;
          idleDuration = 0;
          if (poorPerformanceDuration > 2000) {
            stepDownGovernor(onLevelChange);
            poorPerformanceDuration = 0;
          }
        } else {
          poorPerformanceDuration = 0;
          idleDuration += delta;
          // Step back up only after 30s of healthy idle
          if (idleDuration > 30000 && governorLevel !== 'L1' && governorLevel !== 'L0') {
            governorLevel = 'L1';
            onLevelChange?.(governorLevel);
            idleDuration = 0;
          }
        }
      }
    }

    if (isGovernorRunning) {
      animId = requestAnimationFrame(loop);
    }
  };

  animId = requestAnimationFrame(loop);

  return () => {
    isGovernorRunning = false;
    cancelAnimationFrame(animId);
  };
}

function stepDownGovernor(onLevelChange?: (level: GlassGovernorLevel) => void) {
  if (governorLevel === 'L0') governorLevel = 'L1';
  else if (governorLevel === 'L1') governorLevel = 'L2';
  else if (governorLevel === 'L2') governorLevel = 'L3';

  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-glass-gov', governorLevel);
  }
  onLevelChange?.(governorLevel);
}

export function setForceGovernorLevel(level: GlassGovernorLevel) {
  governorLevel = level;
  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-glass-gov', governorLevel);
  }
}
