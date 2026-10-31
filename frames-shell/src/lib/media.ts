const isWindows = typeof navigator !== 'undefined' ? navigator.userAgent.includes('Windows') : false;
const base = isWindows ? 'http://frames.localhost' : 'frames://localhost';
export const thumbUrl = (id: string, size: 256 | 1024 = 256) => `${base}/thumb/${id}/${size}`;
export const photoUrl = (id: string) => `${base}/photo/${id}`;
