import { display } from './preferences.svelte';

/** Shared bar visibility. Programmatic jumps never masquerade as reading gestures. */
class Chrome {
  visible = $state(true);
  keyboardNavigation = $state(false);
  suspended = false;
  private lastY = 0;

  get shown(): boolean {
    return this.visible || display.keepBars || this.keyboardNavigation;
  }

  show(): void {
    this.visible = true;
  }

  reset(y = 0): void {
    this.lastY = y;
    this.visible = true;
  }

  rebase(y: number): void { this.lastY = y; }

  scroll(y: number): void {
    if (!this.suspended) {
      if (y <= 0 || y < this.lastY) this.visible = true;
      else if (y > this.lastY && !display.keepBars && !this.keyboardNavigation) {
        this.visible = false;
      }
    }
    this.lastY = y;
  }
}

export const chrome = new Chrome();
