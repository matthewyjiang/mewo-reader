<script module lang="ts">
  export type { TimelineAnchor } from '../lib/timeline';
</script>

<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import type { Snippet } from 'svelte';
  import { TimelineLayout } from '../lib/timeline';
  import type { TimelineAnchor } from '../lib/timeline';

  let { keys, scroller, inset = 0, row, onvisible, onadjust }: {
    keys: readonly string[];
    scroller: HTMLElement;
    inset?: number;
    row: Snippet<[number]>;
    onvisible?: (anchor: TimelineAnchor) => void;
    onadjust?: (scrollTop: number) => void;
  } = $props();

  const layout = new TimelineLayout(untrack(() => keys));
  let renderKeys = $state.raw(layout.keys);
  let version = $state(0);
  let start = $state(0);
  let end = $state(0);
  let root = $state<HTMLDivElement>();
  const nodes = new Map<string, HTMLElement>();
  let observer: ResizeObserver;
  let frame = 0;
  let active = true;
  let mounted = false;
  let jumping = false;
  let jumpGeneration = 0;
  let lastAnchor: TimelineAnchor = { key: layout.keys[0] ?? '', index: 0, offset: 0 };

  function measure(): void {
    for (const [key, node] of nodes) {
      if (layout.measure(key, node.getBoundingClientRect().height)) version++;
    }
  }

  function top(): number {
    return root!.getBoundingClientRect().top - scroller.getBoundingClientRect().top
      + scroller.scrollTop;
  }

  export function anchor(): TimelineAnchor {
    if (!root?.isConnected) return lastAnchor;
    const line = scroller.getBoundingClientRect().top + inset;
    for (const [key, node] of nodes) {
      const box = node.getBoundingClientRect();
      const index = layout.indexOf(key);
      if (index >= 0 && box.top <= line && box.bottom > line) {
        lastAnchor = { key, index, offset: line - box.top };
        return lastAnchor;
      }
    }
    const y = Math.max(0, scroller.scrollTop + inset - top());
    const index = layout.indexAt(y);
    lastAnchor = { key: layout.keys[index] ?? '', index, offset: Math.max(0, y - layout.offset(index)) };
    return lastAnchor;
  }

  function windowAt(y: number): void {
    // One viewport on either side tolerates fast scrolling without a fixed row cap.
    const overscan = scroller.clientHeight;
    start = layout.indexAt(Math.max(0, y - overscan));
    end = layout.keys.length
      ? Math.min(layout.keys.length, layout.indexAt(y + scroller.clientHeight + overscan) + 1) : 0;
  }

  function update(report = true): void {
    windowAt(Math.max(0, scroller.scrollTop - top()));
    if (report && !jumping) onvisible?.(anchor());
  }

  // Preserve surviving DOM nodes, their focus, and the visible row when a
  // background load inserts results. Metadata-only updates need no layout work.
  $effect.pre(() => {
    const next = keys;
    untrack(() => {
      if (layout.matches(next)) return;
      const saved = anchor();
      layout.reset(next);
      renderKeys = layout.keys;
      version++;
      if (!mounted) return;
      const generation = ++jumpGeneration;
      jumping = true;
      const index = layout.resolve(saved);
      windowAt(Math.max(0, layout.offset(index) + saved.offset - inset));
      void tick().then(() => {
        if (active && generation === jumpGeneration) void restore(saved);
      });
    });
  });

  function schedule(): void {
    if (frame) return;
    frame = requestAnimationFrame(() => { frame = 0; update(); });
  }

  function observe(node: HTMLElement, key: string) {
    nodes.set(key, node);
    observer?.observe(node);
    return { destroy() { observer?.unobserve(node); nodes.delete(key); } };
  }

  export async function restore(saved: TimelineAnchor): Promise<void> {
    await jump(layout.resolve(saved), layout.indexOf(saved.key) < 0 ? 0 : saved.offset);
  }

  export async function jump(index: number, within = 0): Promise<void> {
    if (!layout.keys.length) return;
    const generation = ++jumpGeneration;
    jumping = true;
    index = Math.min(Math.max(0, index), layout.keys.length - 1);
    // Measuring estimated rows can bring more rows into the render window.
    // Settle that window before aligning the target. A newer jump supersedes it.
    while (active && generation === jumpGeneration) {
      scroller.scrollTop = top() + layout.offset(index) + within - inset;
      update(false);
      await tick();
      if (!active || generation !== jumpGeneration) return;
      measure();
      await tick();
      if (!active || generation !== jumpGeneration) return;
      scroller.scrollTop = top() + layout.offset(index) + within - inset;
      const previousStart = start;
      const previousEnd = end;
      update(false);
      await tick();
      if (start === previousStart && end === previousEnd) break;
    }
    if (generation === jumpGeneration) jumping = false;
  }

  let above = $derived.by(() => { version; return layout.offset(start); });
  let below = $derived.by(() => { version; return layout.offset(renderKeys.length) - layout.offset(end); });
  let indices = $derived(Array.from({ length: end - start }, (_, i) => start + i));

  onMount(() => {
    mounted = true;
    observer = new ResizeObserver(() => {
      const saved = anchor();
      const dataset = renderKeys;
      const scrollTop = scroller.scrollTop;
      const before = layout.offset(saved.index);
      measure();
      const delta = layout.offset(saved.index) - before;
      void tick().then(() => {
        if (!active || jumping || dataset !== renderKeys) return;
        // Do not undo a reading gesture that arrived while measurements settled.
        if (delta && scroller.scrollTop === scrollTop) {
          const y = top() + layout.offset(saved.index) + saved.offset - inset;
          onadjust?.(y);
          scroller.scrollTop = y;
        }
        update();
      });
    });
    nodes.forEach((node) => observer.observe(node));
    observer.observe(scroller);
    update(false);
    scroller.addEventListener('scroll', schedule, { passive: true });
    return () => {
      active = false;
      cancelAnimationFrame(frame);
      observer.disconnect();
      scroller.removeEventListener('scroll', schedule);
    };
  });
</script>

<div bind:this={root} class="virtual-timeline">
  <div style:height={`${above}px`} aria-hidden="true"></div>
  {#each indices as index (renderKeys[index])}
    <div use:observe={renderKeys[index]} data-post-index={index}>{@render row(index)}</div>
  {/each}
  <div style:height={`${below}px`} aria-hidden="true"></div>
</div>

<style>
  .virtual-timeline {
    overflow-anchor: none;
  }
</style>
