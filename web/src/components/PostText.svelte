<script lang="ts">
  import { linkSpans } from '../lib/linkify';

  let { text }: { text: string } = $props();

  function pieces(value: string): { text: string; href: string | null }[] {
    const spans = linkSpans(value);
    const out: { text: string; href: string | null }[] = [];
    let cursor = 0;
    for (const span of spans) {
      if (span.start > cursor) out.push({ text: value.slice(cursor, span.start), href: null });
      out.push({ text: value.slice(span.start, span.end), href: span.href });
      cursor = span.end;
    }
    if (cursor < value.length || out.length === 0) out.push({ text: value.slice(cursor), href: null });
    return out;
  }

  let parts = $derived(pieces(text));

  function openLink(event: MouseEvent): void {
    event.stopPropagation();
  }
</script>

{#each parts as part, index (index)}{#if part.href}<a href={part.href} target="_blank" rel="noopener noreferrer" onclick={openLink}>{part.text}</a>{:else}{part.text}{/if}{/each}
