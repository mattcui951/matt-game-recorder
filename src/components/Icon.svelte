<script lang="ts" module>
  // One family: 24px grid, 1.8 stroke, round joins. Filled glyphs are marked with "F:".
  const PATHS: Record<string, string> = {
    library: '<rect x="3" y="4" width="18" height="12.5" rx="1.5"/><path d="M7.5 20h9"/><path d="M10 7.8v5l4.2-2.5z"/>',
    editor: '<rect x="3" y="6" width="18" height="13" rx="1.5"/><path d="M3 10h18M7.5 6l2 4M12.5 6l2 4M17.5 6l2 4"/>',
    settings: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2.2"/><circle cx="9" cy="17" r="2.2"/>',
    folder: '<path d="M3.5 7.5a1 1 0 0 1 1-1h4.8l2 2h8.2a1 1 0 0 1 1 1v8.5a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z"/>',
    trash: '<path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5M10.5 11v5M13.5 11v5"/>',
    star: '<path d="M12 3.8l2.5 5.1 5.6.8-4 3.9 1 5.6-5.1-2.7-5 2.7.9-5.6-4-3.9 5.6-.8z"/>',
    'star-on': 'F:<path d="M12 3.8l2.5 5.1 5.6.8-4 3.9 1 5.6-5.1-2.7-5 2.7.9-5.6-4-3.9 5.6-.8z"/>',
    search: '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.3-4.3"/>',
    filter: '<path d="M4 5.5h16l-6 7V19l-4-2v-4.5z"/>',
    play: 'F:<path d="M8 5.2v13.6l11-6.8z"/>',
    pause: '<path d="M8.5 5.5v13M15.5 5.5v13" stroke-width="3"/>',
    volume: '<path d="M4 9.5h3.5L12 6v12l-4.5-3.5H4z"/><path d="M15.5 9.2a4 4 0 0 1 0 5.6M18 6.8a7.5 7.5 0 0 1 0 10.4"/>',
    mute: '<path d="M4 9.5h3.5L12 6v12l-4.5-3.5H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>',
    fullscreen: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
    scissors: '<circle cx="6.5" cy="7" r="2.5"/><circle cx="6.5" cy="17" r="2.5"/><path d="M8.7 8.3L20 17M8.7 15.7L20 7"/>',
    bolt: '<path d="M13 3L5.5 13.5h6L10.5 21 18.5 10.5h-6z"/>',
    palette: '<path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.1 0 1.7-.8 1.7-1.6 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.8-1.6 1.7-1.6h2.1a3.5 3.5 0 0 0 3.5-3.5c0-4.3-3.8-7.9-8-7.9z"/><circle cx="7.6" cy="11.2" r="1.1"/><circle cx="10.2" cy="7.4" r="1.1"/><circle cx="14.7" cy="7.6" r="1.1"/>',
    download: '<path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14"/>',
    refresh: '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4h-4"/>',
    globe: '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.4 2.3 3.6 5.1 3.6 8.5s-1.2 6.2-3.6 8.5c-2.4-2.3-3.6-5.1-3.6-8.5S9.6 5.8 12 3.5z"/>',
    gauge: '<path d="M4 17.5a8 8 0 1 1 16 0"/><path d="M12 17.5l4.2-5.2"/>',
    record: 'F:<circle cx="12" cy="12" r="6.5"/>',
    stop: 'F:<rect x="7" y="7" width="10" height="10" rx="1.5"/>',
    monitor: '<rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M9 20h6M12 16v4"/>',
    layout: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M9 4v16"/>',
    minimize: '<path d="M6 12h12"/>',
    maximize: '<rect x="6" y="6" width="12" height="12" rx="1"/>',
    close: '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>',
    'chevron-left': '<path d="M14.5 5.5L8 12l6.5 6.5"/>',
    'chevron-right': '<path d="M9.5 5.5L16 12l-6.5 6.5"/>',
    'chevron-down': '<path d="M6 9.5l6 6 6-6"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    minus: '<path d="M5 12h14"/>',
    undo: '<path d="M9 5L4 10l5 5"/><path d="M4 10h10a5 5 0 0 1 0 10h-3"/>',
    redo: '<path d="M15 5l5 5-5 5"/><path d="M20 10H10a5 5 0 0 0 0 10h3"/>',
    slice: '<path d="M12 3v18"/><rect x="4" y="8" width="5" height="8" rx="1"/><rect x="15" y="8" width="5" height="8" rx="1"/>',
    duplicate: '<rect x="8.5" y="8.5" width="11.5" height="11.5" rx="1.5"/><path d="M15.5 8.5V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v9.5a1 1 0 0 0 1 1h3.5"/>',
    export: '<path d="M12 15V3.5M7.5 8L12 3.5 16.5 8M5 13.5V20h14v-6.5"/>',
    restore: '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 4v4h4"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    grid: '<rect x="4" y="4" width="7" height="7" rx="1"/><rect x="13" y="4" width="7" height="7" rx="1"/><rect x="4" y="13" width="7" height="7" rx="1"/><rect x="13" y="13" width="7" height="7" rx="1"/>',
    rows: '<path d="M4 6.5h16M4 12h16M4 17.5h16"/>',
    camera: '<path d="M4 8.5h3.2l1.8-3h6l1.8 3H20v11H4z"/><circle cx="12" cy="13.5" r="3.3"/>',
    edit: '<path d="M4.5 19.5l3.8-.9L19 7.9 16.1 5 5.4 15.7z"/>',
    disk: '<rect x="3" y="6.5" width="18" height="11" rx="2"/><path d="M7 14h.01M11 14h6"/>',
    image: '<rect x="3.5" y="4.5" width="17" height="15" rx="1.5"/><circle cx="9" cy="10" r="1.8"/><path d="M20.5 16l-5-5-8.5 8.5"/>',
    open: '<path d="M14 4h6v6M20 4l-8.5 8.5M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>',
    // Game events
    swords: '<path d="M4 4l11 11M12.5 17.5l5-5M15 15l4.5 4.5M20 4L9 15M6.5 12.5l5 5M9 15l-4.5 4.5"/>',
    skull: '<path d="M12 3.5a7 7 0 0 0-7 7c0 2.4 1.1 3.9 2.5 5v3h9v-3c1.4-1.1 2.5-2.6 2.5-5a7 7 0 0 0-7-7z"/><path d="M10.5 18.5v2M13.5 18.5v2"/><circle cx="9.3" cy="11" r="1.4" fill="currentColor" stroke="none"/><circle cx="14.7" cy="11" r="1.4" fill="currentColor" stroke="none"/>',
    assist: '<path d="M12 3.5l7 2.8v5c0 4.4-3 7.8-7 9.7-4-1.9-7-5.3-7-9.7v-5z"/><path d="M12 8.5v6M9 11.5h6"/>',
    baron: '<path d="M5 5l3 4.5M19 5l-3 4.5"/><path d="M7 9.5h10v5.5a5 5 0 0 1-10 0z"/><path d="M10 13.5h.01M14 13.5h.01" stroke-width="2.6"/>',
    dragon: '<path d="M12 3c1 4 5.2 5.2 5.2 10.2a5.2 5.2 0 0 1-10.4 0c0-2.6 1.4-3.6 2-5.2.8 1.2 1.5 2 2.6 2.3C10.9 8 11 5.5 12 3z"/>',
    herald: '<path d="M2.5 12S6 6 12 6s9.5 6 9.5 6-3.5 6-9.5 6S2.5 12 2.5 12z"/><circle cx="12" cy="12" r="2.6"/>',
    turret: '<path d="M6.5 20.5h11M8.3 20.5l1-10.5h5.4l1 10.5M7.5 10h9V5.5l-2.2 1.6L12 5 9.7 7.1 7.5 5.5z"/>',
    inhibitor: '<path d="M12 3l6 7-6 11-6-11z"/><path d="M6 10h12"/>',
    trophy: '<path d="M8 4h8v5a4 4 0 0 1-8 0z"/><path d="M8 6H5.2A2.8 2.8 0 0 0 8 10M16 6h2.8A2.8 2.8 0 0 1 16 10M12 13v4M8.5 20.5h7M10 17h4"/>',
    flag: '<path d="M6 21V4M6 4h11l-2.5 4L17 12H6"/>',
  };
</script>

<script lang="ts">
  let { name, size = 18, title }: { name: string; size?: number; title?: string } = $props();
  const raw = $derived(PATHS[name] ?? '');
  const filled = $derived(raw.startsWith('F:'));
  const body = $derived(filled ? raw.slice(2) : raw);
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke={filled ? 'none' : 'currentColor'}
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden={title ? undefined : 'true'}
  role={title ? 'img' : undefined}
>
  {#if title}<title>{title}</title>{/if}
  {@html body}
</svg>
