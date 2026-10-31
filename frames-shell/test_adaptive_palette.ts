import { resolvePalette, setCommandInvoker, type BackendAlbumTheme, type AdaptivePalette } from './src/lib/adaptive.ts';

console.log('Testing resolvePalette across the 4 cases...\n');

// Mock data
const mockManualTheme: BackendAlbumTheme = {
  album_id: 'album-manual',
  mode: 'album',
  source: 'manual',
  bg: '#112233',
  sidebar: '#223344',
  accent: '#FF0055',
  on_bg: '#FFFFFF',
  on_sidebar: '#FFFFFF'
};

const mockAutoTheme: BackendAlbumTheme = {
  album_id: 'album-auto',
  mode: 'album',
  source: 'auto',
  auto_bg: '#0A1020',
  auto_sidebar: '#142030',
  auto_accent: '#0A84FF',
  auto_on_bg: '#FFFFFF',
  auto_on_sidebar: '#FFFFFF'
};

const mockAutoEmptyTheme: BackendAlbumTheme = {
  album_id: 'album-auto-empty',
  mode: 'album',
  source: 'auto',
  auto_bg: null,
  auto_sidebar: null,
  auto_accent: null
};

const mockDynamicTheme: BackendAlbumTheme = {
  album_id: 'album-dynamic',
  mode: 'dynamic',
  source: 'auto'
};

const mockPhotoPalette: AdaptivePalette = {
  bg: '#18181A',
  sidebar: '#222226',
  accent: '#FF453A',
  on_bg: '#FFFFFF',
  on_sidebar: '#FFFFFF'
};

let regenerateCalled = false;

setCommandInvoker(async (cmd: string, args?: Record<string, unknown>) => {
  if (cmd === 'get_album_theme') {
    const id = args?.albumId;
    if (id === 'album-manual') return mockManualTheme;
    if (id === 'album-auto') return mockAutoTheme;
    if (id === 'album-auto-empty') return mockAutoEmptyTheme;
    if (id === 'album-dynamic') return mockDynamicTheme;
    return null;
  }
  if (cmd === 'get_palette') {
    const id = args?.assetId;
    if (id === 'photo-1') return mockPhotoPalette;
    return null;
  }
  if (cmd === 'regenerate_album_palette') {
    regenerateCalled = true;
    return null;
  }
  return null;
});

async function runTests() {
  // Case 1: mode = album, source = manual -> returns manual colours
  const p1 = await resolvePalette('photo-1', 'album-manual');
  console.log('Case 1 (album + manual):', p1);
  if (!p1 || p1.bg !== '#112233' || p1.sidebar !== '#223344' || p1.accent !== '#FF0055') {
    throw new Error('Case 1 failed: expected manual colours');
  }

  // Case 2: mode = album, source = auto (populated) -> returns auto colours
  const p2 = await resolvePalette('photo-1', 'album-auto');
  console.log('Case 2 (album + auto populated):', p2);
  if (!p2 || p2.bg !== '#0A1020' || p2.sidebar !== '#142030' || p2.accent !== '#0A84FF') {
    throw new Error('Case 2 failed: expected auto_* colours');
  }

  // Case 3: mode = album, source = auto (empty) -> triggers regenerate and returns null
  regenerateCalled = false;
  const p3 = await resolvePalette('photo-1', 'album-auto-empty');
  console.log('Case 3 (album + auto empty):', p3, 'regenerate called:', regenerateCalled);
  if (p3 !== null || !regenerateCalled) {
    throw new Error('Case 3 failed: expected null and regenerate_album_palette invoked');
  }

  // Case 4: mode = dynamic -> returns get_palette(assetId)
  const p4 = await resolvePalette('photo-1', 'album-dynamic');
  console.log('Case 4 (dynamic per photo):', p4);
  if (!p4 || p4.bg !== '#18181A' || p4.accent !== '#FF453A') {
    throw new Error('Case 4 failed: expected photo palette from get_palette');
  }

  // Extra check: dynamic with no photo selected (null assetId) returns null
  const p4_null = await resolvePalette(null, 'album-dynamic');
  console.log('Case 4b (dynamic with null asset):', p4_null);
  if (p4_null !== null) {
    throw new Error('Case 4b failed: expected null for null assetId');
  }

  console.log('\nAll 4 resolvePalette cases passed successfully!');
}

runTests().catch((err) => {
  console.error(err);
  process.exit(1);
});
