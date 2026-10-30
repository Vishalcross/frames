import type { AlbumItem, CollectionCard, SyncQueueItem } from '$lib/types';

export const SAMPLE_ALBUMS: AlbumItem[] = [
  {
    id: 'album_favorites',
    title: 'Favorites',
    assetCount: 1420,
    keyAssetId: 'asset_000042',
    dateRange: '2021 – 2026'
  },
  {
    id: 'album_japan',
    title: 'Kyoto & Tokyo Autumn',
    assetCount: 486,
    keyAssetId: 'asset_000108',
    dateRange: 'Nov 2024'
  },
  {
    id: 'album_dolomites',
    title: 'Dolomites Trekking',
    assetCount: 312,
    keyAssetId: 'asset_000215',
    dateRange: 'Jul 2025'
  },
  {
    id: 'album_california',
    title: 'Big Sur Roadtrip',
    assetCount: 654,
    keyAssetId: 'asset_000320',
    dateRange: 'Sep 2025'
  },
  {
    id: 'folder_vacations',
    title: 'Vacations',
    assetCount: 1452,
    keyAssetId: 'asset_000412',
    dateRange: '2023 – 2026',
    isFolder: true,
    children: [
      {
        id: 'album_iceland',
        title: 'Iceland Highlands',
        assetCount: 520,
        keyAssetId: 'asset_000512',
        dateRange: 'Feb 2026'
      },
      {
        id: 'album_amalfi',
        title: 'Amalfi Sunset',
        assetCount: 388,
        keyAssetId: 'asset_000620',
        dateRange: 'Jun 2024'
      }
    ]
  }
];

export const SAMPLE_PINNED_COLLECTIONS: { id: string; title: string; icon: string }[] = [
  { id: 'memories', title: 'Memories', icon: 'sparkles' },
  { id: 'trips', title: 'Trips', icon: 'compass' },
  { id: 'people-pets', title: 'People & Pets', icon: 'users' },
  { id: 'album_japan', title: 'Kyoto & Tokyo', icon: 'album' }
];

export const SAMPLE_MEMORIES: CollectionCard[] = [
  {
    id: 'mem_1',
    title: 'One Year Ago Today',
    subtitle: 'Autumn in Kyoto',
    count: 64,
    keyAssetId: 'asset_000108',
    type: 'memories'
  },
  {
    id: 'mem_2',
    title: 'Alpine Escapes',
    subtitle: 'Dolomites & Swiss Peaks',
    count: 142,
    keyAssetId: 'asset_000215',
    type: 'memories'
  },
  {
    id: 'mem_3',
    title: 'Coastal Sunsets',
    subtitle: 'Pacific Highway',
    count: 98,
    keyAssetId: 'asset_000320',
    type: 'memories'
  },
  {
    id: 'mem_4',
    title: 'Weekend Adventures',
    subtitle: 'Tahoe & Yosemite',
    count: 115,
    keyAssetId: 'asset_000412',
    type: 'memories'
  }
];

export const SAMPLE_PEOPLE: CollectionCard[] = [
  {
    id: 'person_1',
    title: 'Elena & Lucas',
    subtitle: '1,420 photos',
    count: 1420,
    keyAssetId: 'asset_000008',
    type: 'people'
  },
  {
    id: 'person_2',
    title: 'Maya (Golden Retriever)',
    subtitle: '840 photos',
    count: 840,
    keyAssetId: 'asset_000024',
    type: 'people'
  },
  {
    id: 'person_3',
    title: 'Marcus',
    subtitle: '512 photos',
    count: 512,
    keyAssetId: 'asset_000036',
    type: 'people'
  },
  {
    id: 'person_4',
    title: 'Chloe',
    subtitle: '390 photos',
    count: 390,
    keyAssetId: 'asset_000050',
    type: 'people'
  }
];

export const SAMPLE_TRIPS: CollectionCard[] = [
  {
    id: 'trip_1',
    title: 'Japan Journey',
    subtitle: '14 Days · 840 items',
    count: 840,
    keyAssetId: 'asset_000108',
    type: 'trips'
  },
  {
    id: 'trip_2',
    title: 'Northern Lights Iceland',
    subtitle: '7 Days · 520 items',
    count: 520,
    keyAssetId: 'asset_000512',
    type: 'trips'
  },
  {
    id: 'trip_3',
    title: 'Italian Lakes & Dolomites',
    subtitle: '10 Days · 610 items',
    count: 610,
    keyAssetId: 'asset_000215',
    type: 'trips'
  }
];

export const SAMPLE_SYNC_QUEUE: SyncQueueItem[] = [
  {
    id: 'sync_1',
    filename: 'IMG_4901.MOV',
    progress: 74,
    totalBytes: 420_000_000,
    uploadedBytes: 310_800_000,
    status: 'uploading'
  },
  {
    id: 'sync_2',
    filename: 'IMG_4902.DNG',
    progress: 15,
    totalBytes: 52_000_000,
    uploadedBytes: 7_800_000,
    status: 'queued'
  },
  {
    id: 'sync_3',
    filename: 'IMG_4903.HEIC',
    progress: 0,
    totalBytes: 4_200_000,
    uploadedBytes: 0,
    status: 'queued'
  }
];
