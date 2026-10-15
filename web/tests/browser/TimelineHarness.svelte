<script lang="ts">
  import { tick } from 'svelte';
  import Timeline from '../../src/components/Timeline.svelte';

  let scroller = $state<HTMLDivElement>();
  let timeline = $state<ReturnType<typeof Timeline>>();
  let keys = $state(Array.from({ length: 40 }, (_, i) => String(i)));

  export async function updateAndJump() {
    keys = ['new', ...keys];
    await tick();
    await timeline!.jump(25);
  }

  export function anchor() { return timeline!.anchor(); }
</script>

<div bind:this={scroller} style="height: 600px; overflow-y: auto">
  {#if scroller}
    <Timeline bind:this={timeline} {keys} {scroller}>
      {#snippet row(index: number)}<div style="height: 80px">{keys[index]}</div>{/snippet}
    </Timeline>
  {/if}
</div>
