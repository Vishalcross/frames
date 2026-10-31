import { isTauriAvailable } from '$lib/services/tauriRepo';

const isWindows = typeof navigator !== 'undefined' ? navigator.userAgent.includes('Windows') : false;
const base = isWindows ? 'http://frames.localhost' : 'frames://localhost';

export const thumbUrl = (id: string, size: 256 | 1024 = 256) => `${base}/thumb/${id}/${size}`;
export const photoUrl = (id: string) => `${base}/photo/${id}`;

type ThumbReadyCallback = (assetId: string) => void;
const thumbReadyListeners = new Set<ThumbReadyCallback>();
let unlistenThumbReady: (() => void) | null = null;

export function onThumbReady(callback: ThumbReadyCallback): () => void {
  thumbReadyListeners.add(callback);
  if (thumbReadyListeners.size === 1 && isTauriAvailable()) {
    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<{ asset_id: string }>('thumb-ready', (event) => {
        if (event.payload?.asset_id) {
          thumbReadyListeners.forEach((cb) => cb(event.payload.asset_id));
        }
      }).then((unlisten) => {
        unlistenThumbReady = unlisten;
      });
    });
  }
  return () => {
    thumbReadyListeners.delete(callback);
    if (thumbReadyListeners.size === 0 && unlistenThumbReady) {
      unlistenThumbReady();
      unlistenThumbReady = null;
    }
  };
}

let visibleAssetIds = new Set<string>();
let prioritizeTimer: any = null;

function schedulePrioritize() {
  if (prioritizeTimer) clearTimeout(prioritizeTimer);
  prioritizeTimer = setTimeout(() => {
    if (visibleAssetIds.size > 0 && isTauriAvailable()) {
      import('@tauri-apps/api/core').then(({ invoke }) => {
        invoke('prioritize', { assetIds: Array.from(visibleAssetIds) }).catch(() => {});
      });
    }
  }, 150);
}

let sharedObserver: IntersectionObserver | null = null;
function getSharedObserver() {
  if (!sharedObserver && typeof IntersectionObserver !== 'undefined') {
    sharedObserver = new IntersectionObserver(
      (entries) => {
        let changed = false;
        for (const entry of entries) {
          const id = (entry.target as HTMLElement).dataset.assetId;
          if (!id) continue;
          if (entry.isIntersecting) {
            if (!visibleAssetIds.has(id)) {
              visibleAssetIds.add(id);
              changed = true;
            }
          } else {
            if (visibleAssetIds.delete(id)) {
              changed = true;
            }
          }
        }
        if (changed) schedulePrioritize();
      },
      { threshold: 0.05 }
    );
  }
  return sharedObserver;
}

export function observeAssetVisibility(node: HTMLElement, assetId: string) {
  node.dataset.assetId = assetId;
  const observer = getSharedObserver();
  observer?.observe(node);
  return {
    update(newId: string) {
      node.dataset.assetId = newId;
    },
    destroy() {
      observer?.unobserve(node);
      if (visibleAssetIds.delete(node.dataset.assetId || '')) {
        schedulePrioritize();
      }
    }
  };
}
