/**
 * Interleaved A/B benchmarking.
 *
 * Wall clock on these machines drifts several percent over minutes --- larger
 * than most changes worth making --- so two back-to-back blocks cannot be
 * compared. Variants alternate A,B,A,B... over several rounds and are compared
 * pairwise within a round. Spread *within* a round is tight and therefore
 * falsely reassuring; it measures repeatability, not drift.
 */

export interface Sample {
  variant: string;
  round: number;
  ms: number;
  /** Arbitrary per-sample counters, summed and reported alongside timings. */
  counters: Record<string, number>;
}

export interface VariantStats {
  variant: string;
  n: number;
  median: number;
  min: number;
  max: number;
  counters: Record<string, number>;
}

export interface PairwiseResult {
  a: string;
  b: string;
  /** Per-round ratio b/a, so <1 means b is faster. */
  ratios: number[];
  medianRatio: number;
}

export interface BenchResult {
  stats: VariantStats[];
  pairwise: PairwiseResult[];
  samples: Sample[];
}

export type Variant = () => Promise<Record<string, number>>;

/**
 * Runs each variant once per round, in order, for `rounds` rounds.
 *
 * Returns per-variant stats plus pairwise per-round ratios against the first
 * variant, which is the only comparison that survives clock drift.
 */
export async function interleaved(
  variants: Record<string, Variant>,
  rounds: number,
): Promise<BenchResult> {
  const names = Object.keys(variants);
  const samples: Sample[] = [];

  for (let round = 0; round < rounds; round++) {
    for (const name of names) {
      const run = variants[name];
      if (!run) continue;
      const t0 = performance.now();
      const counters = await run();
      samples.push({ variant: name, round, ms: performance.now() - t0, counters });
    }
  }

  const stats = names.map((name) => {
    const mine = samples.filter((s) => s.variant === name);
    const times = mine.map((s) => s.ms).sort((x, y) => x - y);
    const counters: Record<string, number> = {};
    for (const sample of mine) {
      for (const [key, value] of Object.entries(sample.counters)) {
        counters[key] = (counters[key] ?? 0) + value;
      }
    }
    for (const key of Object.keys(counters)) {
      counters[key] = (counters[key] ?? 0) / Math.max(mine.length, 1);
    }
    return {
      variant: name,
      n: times.length,
      median: median(times),
      min: times[0] ?? NaN,
      max: times[times.length - 1] ?? NaN,
      counters,
    };
  });

  const baseline = names[0];
  const pairwise: PairwiseResult[] = [];
  if (baseline !== undefined) {
    for (const name of names.slice(1)) {
      const ratios: number[] = [];
      for (let round = 0; round < rounds; round++) {
        const a = samples.find((s) => s.variant === baseline && s.round === round);
        const b = samples.find((s) => s.variant === name && s.round === round);
        if (a && b && a.ms > 0) ratios.push(b.ms / a.ms);
      }
      pairwise.push({
        a: baseline,
        b: name,
        ratios,
        medianRatio: median([...ratios].sort((x, y) => x - y)),
      });
    }
  }

  return { stats, pairwise, samples };
}

function median(sorted: number[]): number {
  if (sorted.length === 0) return NaN;
  const mid = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 1) return sorted[mid] ?? NaN;
  return ((sorted[mid - 1] ?? NaN) + (sorted[mid] ?? NaN)) / 2;
}
