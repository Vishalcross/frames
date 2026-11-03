import { thumbHashToDataURL } from 'thumbhash';

const isWindows = typeof navigator !== 'undefined' ? navigator.userAgent.includes('Windows') : false;
const base = isWindows ? 'http://frames.localhost' : 'frames://localhost';
export const thumbUrl = (id: string, size: 256 | 1024 = 256) => `${base}/thumb/${id}/${size}`;
export const photoUrl = (id: string) => `${base}/photo/${id}`;
/** A Live Photo's motion clip (its MOV). */
export const motionUrl = (id: string) => `${base}/motion/${id}`;

/** Whether this web view decodes HEVC (always on macOS; on Windows only with Microsoft's add-on). */
export function canPlayHevc(): boolean {
  if (typeof document === 'undefined') return false;
  const v = document.createElement('video');
  return ['video/mp4; codecs="hvc1"', 'video/mp4; codecs="hev1"'].some((t) => v.canPlayType(t) !== '');
}

/**
 * A blurred sketch of a photo from its ThumbHash, painted while the real
 * thumbnail loads (Google Photos' low-res placeholders, at ~25 bytes each).
 * Decoded once per hash; '' when there is none.
 */
const placeholderCache = new Map<string, string>();
export function placeholderUrl(thumbhash: string | undefined): string {
  if (!thumbhash) return '';
  let url = placeholderCache.get(thumbhash);
  if (url === undefined) {
    try {
      const bytes = Uint8Array.from(atob(thumbhash), (c) => c.charCodeAt(0));
      url = thumbHashToDataURL(bytes);
    } catch {
      url = '';
    }
    if (placeholderCache.size > 5000) placeholderCache.clear();
    placeholderCache.set(thumbhash, url);
  }
  return url;
}
