/** Keyed variable-height layout. Fenwick sums keep resize and seek O(log n). */
export class VirtualLayout {
  keys: string[] = [];
  private positions = new Map<string, number>();
  private measured = new Map<string, number>();
  private heights: number[] = [];
  private tree: number[] = [];
  reset(keys: string[], estimate: (key: string) => number = () => 76) {
    this.keys = keys;
    this.positions = new Map(keys.map((key, i) => [key, i]));
    this.heights = keys.map((key) => this.measured.get(key) ?? estimate(key));
    this.tree = new Array(keys.length + 1).fill(0);
    for (let i = 0; i < keys.length; i++) this.add(i, this.heights[i]);
  }
  clear() {
    this.measured.clear();
    this.reset([]);
  }
  private add(index: number, delta: number) {
    for (let i = index + 1; i < this.tree.length; i += i & -i) this.tree[i] += delta;
  }
  measure(key: string, height: number): boolean {
    const i = this.positions.get(key);
    if (i === undefined || !Number.isFinite(height) || height <= 0) return false;
    this.measured.set(key, height);
    const delta = height - this.heights[i];
    if (Math.abs(delta) < 0.5) return false;
    this.heights[i] = height;
    this.add(i, delta);
    return true;
  }
  offset(index: number): number {
    let sum = 0;
    for (let i = Math.min(index, this.keys.length); i > 0; i -= i & -i) sum += this.tree[i];
    return sum;
  }
  get total() {
    return this.offset(this.keys.length);
  }
  index(key: string) {
    return this.positions.get(key);
  }
  indexAt(offset: number): number {
    let index = 0,
      sum = 0;
    let bit = 1;
    while (bit * 2 < this.tree.length) bit *= 2;
    for (; bit; bit >>= 1) {
      const next = index + bit;
      if (next < this.tree.length && sum + this.tree[next] <= offset) {
        sum += this.tree[next];
        index = next;
      }
    }
    return Math.min(index, Math.max(0, this.keys.length - 1));
  }
  range(top: number, height: number, overscan = 600) {
    if (!this.keys.length) return { start: 0, end: 0, before: 0, after: 0 };
    const start = this.indexAt(Math.max(0, top - overscan));
    const end = Math.min(this.keys.length, this.indexAt(top + height + overscan) + 1);
    return { start, end, before: this.offset(start), after: this.total - this.offset(end) };
  }
  anchor(top: number) {
    const index = this.indexAt(top);
    return { key: this.keys[index], within: top - this.offset(index) };
  }
  restore(anchor: { key: string | undefined; within: number }): number | undefined {
    const i = anchor.key === undefined ? undefined : this.index(anchor.key);
    return i === undefined ? undefined : this.offset(i) + anchor.within;
  }
}

export function localDateBounds(value: string): { start: string; end: string } | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const start = new Date(`${value}T00:00:00`);
  if (!Number.isFinite(start.getTime())) return null;
  const [year, month, day] = value.split("-").map(Number);
  if (start.getFullYear() !== year || start.getMonth() + 1 !== month || start.getDate() !== day)
    return null;
  const end = new Date(start);
  end.setDate(end.getDate() + 1);
  return {
    start: `${Math.floor(start.getTime() / 1000)}.000000`,
    end: `${Math.floor(end.getTime() / 1000)}.000000`,
  };
}
