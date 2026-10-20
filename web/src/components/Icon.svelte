<script module lang="ts">
  export type IconName =
    | 'home' | 'search' | 'heart' | 'comment' | 'quote' | 'share'
    | 'back' | 'close' | 'plus' | 'trash' | 'settings' | 'chevron'
    | 'more' | 'book' | 'lock' | 'people' | 'refresh' | 'person' | 'repost' | 'shield'
    | 'clock' | 'check' | 'retry';
  const paths: Record<IconName, string> = {
    home: 'M3 10 12 3l9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z',
    search: 'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',
    heart: 'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1'
      + 'a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z',
    comment: 'M21 11.5a8.5 8.5 0 0 1-8.5 8.5H4l-2 2V11.5a9.5 9.5 0 0 1 19 0Z',
    // Material Icons Outlined format_quote. A fill glyph, same as Android.
    quote: 'M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z',
    share: 'M12 16V2m0 0-5 5m5-5 5 5M5 12H3v9h18v-9h-2',
    back: 'M20 12H4m0 0 7-7m-7 7 7 7',
    close: 'm6 6 12 12M6 18 18 6',
    plus: 'M12 4v16M4 12h16',
    trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7',
    settings: 'm9 3-1 3-3 1-2 3 2 2-1 4 3 2 3-1 3 2 3-2 1-3 3-2-1-4-3-1-1-3Z'
      + 'm7 9a4 4 0 1 1-8 0 4 4 0 0 1 8 0',
    chevron: 'm9 5 7 7-7 7',
    more: 'M4 12h.01M12 12h.01M20 12h.01',
    book: 'M12 5C8 2 4 3 2 4v16c4-2 7-1 10 1m0-16c4-3 8-2 10-1v16'
      + 'c-4-2-7-1-10 1V5',
    // Material Icons Outlined lock. Fill glyph (shackle cutout and keyhole are subpaths).
    lock: 'M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12'
      + 'c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zM9 6c0-1.66 1.34-3 3-3s3 1.34 3 3v2H9V6z'
      + 'm9 14H6V10h12v10zm-6-3c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2z',
    // Material Icons Filled people.
    refresh: 'M21 12a9 9 0 0 1-15.5 6.2M3 12A9 9 0 0 1 18.5 5.8M21 4v5h-5M3 20v-5h5',
    people: 'M16 11c1.66 0 2.99-1.34 2.99-3S17.66 5 16 5c-1.66 0-3 1.34-3 3s1.34 3 3 3z'
      + 'm-8 0c1.66 0 2.99-1.34 2.99-3S9.66 5 8 5C6.34 5 5 6.34 5 8s1.34 3 3 3zm0 2c-2.33 0-7'
      + ' 1.17-7 3.5V19h14v-2.5c0-2.33-4.67-3.5-7-3.5zm8 0c-.29 0-.62.02-.97.05'
      + ' 1.16.84 1.97 1.97 1.97 3.45V19h6v-2.5c0-2.33-4.67-3.5-7-3.5z',
    person: 'M15.2 8a3.2 3.2 0 1 1-6.4 0 3.2 3.2 0 0 1 6.4 0M5.5 19.4c.6-3.3 3.2-5.2 6.5-5.2s5.9 1.9 6.5 5.2',
    repost: 'M17 2l4 4-4 4M21 6H9a4 4 0 0 0-4 4v1M7 22l-4-4 4-4M3 18h12a4 4 0 0 0 4-4v-1',
    shield: 'M12 3.2 5 6.4v4.8c0 4.2 2.9 8.1 7 9.4 4.1-1.3 7-5.2 7-9.4V6.4L12 3.2z',
    clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18M12 7v5l3 2',
    check: 'm5 12.5 4.5 4.5L19 7',
    retry: 'M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5',
  };
  const filledPaths: Partial<Record<IconName, string>> = {
    person: 'M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z',
    shield: 'M12 2.2 4.5 5.6v5.2c0 4.7 3.2 9.1 7.5 10.5 4.3-1.4 7.5-5.8 7.5-10.5V5.6L12 2.2z',
  };

  // Outlined lock's artwork is half a unit high in the 24px grid (pad 1 above, 2 below).
  const origin: Partial<Record<IconName, string>> = {
    lock: 'translate(0 0.5)',
  };
  const filledGlyphs = new Set<IconName>(['quote']);
</script>

<script lang="ts">
  let { name, filled = false, size = 22 }: {
    name: IconName;
    filled?: boolean;
    size?: number;
  } = $props();
  let glyph = $derived(filled && filledPaths[name] ? filledPaths[name] : paths[name]);
  let solid = $derived(filledGlyphs.has(name) || (filled && !!filledPaths[name]));
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
  aria-hidden="true"
>
  {#if origin[name]}
    <g transform={origin[name]}>
      <path d={glyph} fill={solid ? 'currentColor' : undefined} stroke={solid ? 'none' : undefined} />
    </g>
  {:else}
    <path
      d={glyph}
      fill={solid && !filled ? 'currentColor' : undefined}
      stroke={solid && !filled ? 'none' : undefined}
    />
  {/if}
</svg>
