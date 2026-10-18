/**
 * One generation per book. A like or repost bumps it, and a load that started
 * earlier is not allowed to publish. Mutations share a queue so a load that
 * starts during one waits until the server answer is written.
 */
export class SocialGate {
  generation = 0;
  private requests = 0;
  private tail: Promise<void> = Promise.resolve();

  bump(): number {
    this.generation += 1;
    return this.generation;
  }

  /** A load may publish only while this ticket still matches. */
  begin(): { generation: number; request: number } {
    this.requests += 1;
    return { generation: this.generation, request: this.requests };
  }

  current(ticket: { generation: number; request: number }): boolean {
    return ticket.generation === this.generation && ticket.request === this.requests;
  }

  async wait(): Promise<void> {
    let pending = this.tail;
    for (;;) {
      await pending;
      if (pending === this.tail) return;
      pending = this.tail;
    }
  }

  enqueue<T>(work: () => Promise<T>): Promise<T> {
    const run = this.tail.then(work, work);
    const settled = run.then(() => {}, () => {});
    this.tail = settled;
    return run;
  }
}

export class SocialBooks {
  private gates = new Map<string, SocialGate>();

  gate(id: string): SocialGate {
    let found = this.gates.get(id);
    if (!found) {
      found = new SocialGate();
      this.gates.set(id, found);
    }
    return found;
  }

  clear(): void {
    this.gates.clear();
  }
}

/**
 * Read after any in-flight mutation. The result is dropped when a repost or
 * like bumped the generation while the read was running.
 */
export async function loadSocialIndex<T>(
  gate: SocialGate,
  read: () => Promise<T>,
): Promise<T | null> {
  await gate.wait();
  const ticket = gate.begin();
  const value = await read();
  return gate.current(ticket) ? value : null;
}

/**
 * Invalidate loads that already captured a generation, then hold later loads
 * until [commit] finishes. The bump is synchronous: the optimistic write is
 * queued, and a load whose response is already pending must not publish first.
 */
export function startRepost(gate: SocialGate, commit: () => Promise<void>): Promise<void> {
  gate.bump();
  return gate.enqueue(commit);
}
