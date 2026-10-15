<script module lang="ts">
  export interface TimelineAnchor { index: number; offset: number }
</script>

<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import type { Snippet } from 'svelte';

  let { count, scroller, inset = 0, row, onvisible, onadjust }: {
    count: number;
    scroller: HTMLElement;
    inset?: number;
    row: Snippet<[number]>;
    onvisible?: (anchor: TimelineAnchor) => void;
    onadjust?: (scrollTop: number) => void;
  } = $props();

  // Estimated heights live in a Fenwick tree: offset/jump lookup is logarithmic,
  // while only the viewport and one viewport of overscan create DOM/components.
  // 192px is the previous feed's four-line-plus-chrome intrinsic height estimate.
  const estimate = 192;
  const heights = new Map<number, number>();
  // The owning keyed destination remounts when its dataset changes.
  const size = untrack(() => count);
  const tree = new Float64Array(size + 1);
  for (let i = 1; i <= size; i++) tree[i] = (i & -i) * estimate;
  let version = $state(0);
  let start = $state(0);
  let end = $state(0);
  let root = $state<HTMLDivElement>();
  const nodes = new Map<number, HTMLElement>();
  let observer: ResizeObserver;
  let frame = 0;
  let active = true;
  let jumping = false;
  let lastAnchor: TimelineAnchor = { index: 0, offset: 0 };

  function offset(index: number): number {
    let total = 0;
    for (let i = index; i > 0; i -= i & -i) total += tree[i];
    return total;
  }

  function indexAt(y: number): number {
    let index = 0;
    let total = 0;
    let bit = 1;
    while (bit * 2 <= count) bit *= 2;
    for (; bit; bit >>= 1) {
      const next = index + bit;
      if (next <= count && total + tree[next] <= y) {
        index = next;
        total += tree[next];
      }
    }
    return Math.min(index, Math.max(0, count - 1));
  }

  function measure(index: number, height: number): void {
    if (!height) return;
    const delta = height - (heights.get(index) ?? estimate);
    if (!delta) return;
    heights.set(index, height);
    for (let i = index + 1; i <= count; i += i & -i) tree[i] += delta;
    version++;
  }

  function top(): number {
    return root!.getBoundingClientRect().top - scroller.getBoundingClientRect().top
      + scroller.scrollTop;
  }

  export function anchor(): TimelineAnchor {
    if (!root?.isConnected) return lastAnchor;
    const line = scroller.getBoundingClientRect().top + inset;
    for (const [index, node] of nodes) {
      const box = node.getBoundingClientRect();
      if (box.top <= line && box.bottom > line) {
        lastAnchor = { index, offset: line - box.top };
        return lastAnchor;
      }
    }
    const y = Math.max(0, scroller.scrollTop + inset - top());
    const index = indexAt(y);
    lastAnchor = { index, offset: Math.max(0, y - offset(index)) };
    return lastAnchor;
  }

  function update(report = true): void {
    const y = Math.max(0, scroller.scrollTop - top());
    // One viewport on either side tolerates fast scrolling without a fixed row cap.
    const overscan = scroller.clientHeight;
    start = indexAt(Math.max(0, y - overscan));
    end = count ? Math.min(count, indexAt(y + scroller.clientHeight + overscan) + 1) : 0;
    if (report && !jumping) onvisible?.(anchor());
  }

  function schedule(): void {
    if (frame) return;
    frame = requestAnimationFrame(() => { frame = 0; update(); });
  }

  function observe(node: HTMLElement, index: number) {
    nodes.set(index, node);
    observer?.observe(node);
    return { destroy() { observer?.unobserve(node); nodes.delete(index); } };
  }

  export async function jump(index: number, within = 0): Promise<void> {
    if (!count) return;
    jumping = true;
    index = Math.min(Math.max(0, index), count - 1);
    // Measuring estimated rows can bring more rows into the render window.
    // Settle that window before the final alignment, or a late measurement
    // above the target can move the restored row by an entire paragraph.
    while (active) {
      scroller.scrollTop = top() + offset(index) + within - inset;
      update(false);
      await tick();
      if (!active) return;
      for (const [i, node] of nodes) measure(i, node.getBoundingClientRect().height);
      await tick();
      if (!active) return;
      scroller.scrollTop = top() + offset(index) + within - inset;
      const previousStart = start;
      const previousEnd = end;
      update(false);
      await tick();
      if (start === previousStart && end === previousEnd) break;
    }
    jumping = false;
  }

  let above = $derived.by(() => { version; return offset(start); });
  let below = $derived.by(() => { version; return offset(count) - offset(end); });
  let indices = $derived(Array.from({ length: end - start }, (_, i) => start + i));

  onMount(() => {
    observer = new ResizeObserver(() => {
      const saved = anchor();
      const scrollTop = scroller.scrollTop;
      const before = offset(saved.index);
      for (const [i, node] of nodes) measure(i, node.getBoundingClientRect().height);
      const delta = offset(saved.index) - before;
      void tick().then(() => {
        if (!active || jumping) return;
        // Do not undo a reading gesture that arrived while measurements settled.
        if (delta && scroller.scrollTop === scrollTop) {
          const y = top() + offset(saved.index) + saved.offset - inset;
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
  {#each indices as index (index)}
    <div use:observe={index} data-post-index={index}>{@render row(index)}</div>
  {/each}
  <div style:height={`${below}px`} aria-hidden="true"></div>
</div>

<style>
  .virtual-timeline {
    overflow-anchor: none;
  }
</style>
