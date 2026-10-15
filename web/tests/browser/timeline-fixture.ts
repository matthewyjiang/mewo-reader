import { mount, tick, unmount } from 'svelte';
import TimelineHarness from './TimelineHarness.svelte';

export async function updateAndJump() {
  const target = document.createElement('div');
  target.style.cssText = 'position:fixed;inset:0;background:white';
  document.body.append(target);
  const harness = mount(TimelineHarness, { target });
  try {
    await tick();
    await harness.updateAndJump();
    await new Promise(requestAnimationFrame);
    return harness.anchor();
  } finally {
    await unmount(harness);
    target.remove();
  }
}
