import { readStored, writeStored } from './storage';

/** Keep the latest value until acknowledged. Disk survives lost keepalive requests;
 * background writes bypass in-flight work, which re-sends any newer value afterward. */
export class PendingProgress {
  private account = '';
  private pending = new Map<string, number>();
  private timers = new Map<string, ReturnType<typeof setTimeout>>();
  private flights = new Map<string, Promise<void>>();
  private versions = new Map<string, number>();
  private requests = new Map<string, Set<{ version: number | undefined }>>();
  private generation = 0;

  constructor(
    private send: (id: string, index: number, keepalive: boolean) => Promise<void>,
    private failed: (error: unknown) => void,
  ) {}

  start(account: string): void {
    this.clear();
    this.account = account;
    const saved = readStored<Record<string, number>>(this.key(), {});
    for (const [id, index] of Object.entries(saved ?? {})) {
      if (Number.isInteger(index) && index >= 0) this.pending.set(id, index);
    }
  }

  private key(): string { return `mewo.progress.${this.account}`; }
  private persist(): void {
    if (this.account) writeStored(this.key(), Object.fromEntries(this.pending));
  }

  entries(): IterableIterator<[string, number]> { return this.pending.entries(); }
  has(id: string): boolean { return this.pending.has(id); }

  save(id: string, index: number): void {
    if (this.pending.get(id) !== index) {
      this.versions.set(id, (this.versions.get(id) ?? 0) + 1);
    }
    this.pending.set(id, index);
    this.persist();
    clearTimeout(this.timers.get(id));
    // Match the existing 200ms chrome transition to coalesce scrolling writes.
    this.timers.set(id, setTimeout(() => this.flush(id), 200));
  }

  remove(id: string): void {
    this.pending.delete(id);
    this.versions.delete(id);
    clearTimeout(this.timers.get(id));
    this.timers.delete(id);
    this.persist();
  }

  flush(onlyId?: string, background = false): void {
    for (const [id, index] of this.pending) {
      if (onlyId && onlyId !== id) continue;
      clearTimeout(this.timers.get(id));
      this.timers.delete(id);
      if (!background && this.flights.has(id)) continue;
      const generation = this.generation;
      const version = this.versions.get(id);
      const requests = this.requests.get(id) ?? new Set<{ version: number | undefined }>();
      const attempt = { version };
      requests.add(attempt);
      this.requests.set(id, requests);
      const request = this.send(id, index, background).then(() => {
        if (generation !== this.generation) return;
        // Any older request may still overwrite a concurrent keepalive.
        const older = [...requests].some((item) => item.version !== version);
        if (!older && this.pending.get(id) === index
          && this.versions.get(id) === version) {
          this.pending.delete(id);
          this.persist();
        }
      }).catch((error: unknown) => {
        if (generation === this.generation && this.pending.has(id)) this.failed(error);
      }).finally(() => {
        if (generation !== this.generation) return;
        requests.delete(attempt);
        if (!requests.size) this.requests.delete(id);
        if (!background) this.flights.delete(id);
        // Retry a newer value immediately, but leave failed identical writes pending.
        const latest = this.pending.get(id);
        if (latest !== undefined && this.versions.get(id) !== version) {
          this.flush(id, background);
        }
      });
      if (!background) this.flights.set(id, request);
    }
  }

  async settle(): Promise<void> {
    this.flush();
    // Newer values can spawn another flight when an earlier request completes.
    while (this.flights.size) await Promise.all(this.flights.values());
  }

  clear(): void {
    this.generation++;
    this.timers.forEach(clearTimeout);
    this.timers.clear();
    this.pending.clear();
    this.flights.clear();
    this.versions.clear();
    this.requests.clear();
    this.account = '';
  }
}
