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
