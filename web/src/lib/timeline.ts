export interface TimelineAnchor { key: string; index: number; offset: number }

/** Keyed height measurements and logarithmic row/offset lookup for a virtual feed. */
export class TimelineLayout {
  private rowKeys: readonly string[] = [];
  get keys(): readonly string[] { return this.rowKeys; }
  private positions = new Map<string, number>();
  private heights = new Map<string, number>();
  private tree = new Float64Array(1);
  // The existing feed estimate is a four-line post plus its chrome.
  private readonly estimate = 192;

  constructor(keys: readonly string[]) { this.reset(keys); }

  matches(keys: readonly string[]): boolean {
    return keys.length === this.keys.length && keys.every((key, i) => key === this.keys[i]);
  }

  /** Keep measurements for surviving rows when results are inserted or removed. */
  reset(keys: readonly string[]): void {
    const positions = new Map(keys.map((key, index) => [key, index]));
    if (positions.size !== keys.length) throw new Error('Timeline row keys must be unique.');
    this.rowKeys = [...keys];
    this.positions = positions;
    this.heights = new Map([...this.heights].filter(([key]) => this.positions.has(key)));
    this.tree = new Float64Array(keys.length + 1);
    for (let i = 1; i <= keys.length; i++) {
      this.tree[i] += this.heights.get(keys[i - 1]) ?? this.estimate;
      const parent = i + (i & -i);
      if (parent <= keys.length) this.tree[parent] += this.tree[i];
    }
  }

  offset(index: number): number {
    let total = 0;
    for (let i = index; i > 0; i -= i & -i) total += this.tree[i];
    return total;
  }

  indexAt(y: number): number {
    let index = 0;
    let total = 0;
    let bit = 1;
    while (bit * 2 <= this.keys.length) bit *= 2;
    for (; bit; bit >>= 1) {
      const next = index + bit;
      if (next <= this.keys.length && total + this.tree[next] <= y) {
        index = next;
        total += this.tree[next];
      }
    }
    return Math.min(index, Math.max(0, this.keys.length - 1));
  }

  indexOf(key: string): number { return this.positions.get(key) ?? -1; }

  /** Use identity first, then the nearest surviving position if a row was removed. */
  resolve(anchor: TimelineAnchor): number {
    return this.positions.get(anchor.key)
      ?? Math.min(anchor.index, Math.max(0, this.keys.length - 1));
  }

  measure(key: string, height: number): boolean {
    const index = this.positions.get(key);
    if (index === undefined || !height) return false;
    const delta = height - (this.heights.get(key) ?? this.estimate);
    if (!delta) return false;
    this.heights.set(key, height);
    for (let i = index + 1; i <= this.keys.length; i += i & -i) this.tree[i] += delta;
    return true;
  }
}
