<script module lang="ts">
  export type IconName =
    | 'home' | 'search' | 'heart' | 'comment' | 'quote' | 'share'
    | 'back' | 'close' | 'plus' | 'trash' | 'settings' | 'chevron'
    | 'more' | 'book';
  const paths: Record<IconName, string> = {
    home: 'M3 10 12 3l9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z',
    search: 'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',
    heart: 'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1'
      + 'a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z',
    comment: 'M21 11.5a8.5 8.5 0 0 1-8.5 8.5H4l-2 2V11.5a9.5 9.5 0 0 1 19 0Z',
    quote: 'M4 8h14l-3-3m3 3-3 3M20 16H6l3 3m-3-3 3-3M4 8v6m16 2v-6',
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
  };
</script>

<script lang="ts">
  let { name, filled = false, size = 22 }: {
    name: IconName;
    filled?: boolean;
    size?: number;
  } = $props();
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
  <path d={paths[name]} />
</svg>
