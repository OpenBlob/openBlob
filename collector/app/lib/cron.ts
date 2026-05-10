/**
 * Tiny 5-field cron parser used to compute the next run timestamp of the
 * server-side bundling job, so the UI can show a countdown.
 *
 * Fields, in order: minute hour day-of-month month day-of-week.
 *
 * Supported syntax per field:
 *   *           every value in range
 *   N           a specific value
 *   N,M,...     a list
 *   N-M         an inclusive range
 *   * /S        every S-th value (S > 0)
 *   N-M/S       every S-th value within a range
 *
 * Day-of-week accepts 0-7 with both 0 and 7 meaning Sunday. When both
 * day-of-month and day-of-week are restricted (i.e. neither is `*`), Vixie
 * cron semantics apply: a day matches if either field matches.
 *
 * All times are interpreted in UTC to match `Deno.cron`.
 */

type Field = Set<number>;

type ParsedCron = {
  minute: Field;
  hour: Field;
  dom: Field;
  month: Field;
  dow: Field;
  domRestricted: boolean;
  dowRestricted: boolean;
};

function parseField(spec: string, min: number, max: number): Field {
  const out = new Set<number>();
  for (const part of spec.split(",")) {
    let range = part;
    let step = 1;
    const slashIdx = range.indexOf("/");
    if (slashIdx !== -1) {
      step = Number(range.slice(slashIdx + 1));
      range = range.slice(0, slashIdx);
      if (!Number.isInteger(step) || step <= 0) {
        throw new Error(`Invalid cron step in "${part}"`);
      }
    }
    let lo: number;
    let hi: number;
    if (range === "*") {
      lo = min;
      hi = max;
    } else if (range.includes("-")) {
      const [a, b] = range.split("-");
      lo = Number(a);
      hi = Number(b);
    } else {
      lo = Number(range);
      hi = lo;
    }
    if (!Number.isInteger(lo) || !Number.isInteger(hi) || lo < min || hi > max || lo > hi) {
      throw new Error(`Invalid cron range "${part}" (allowed ${min}-${max})`);
    }
    for (let v = lo; v <= hi; v += step) out.add(v);
  }
  if (out.size === 0) throw new Error(`Empty cron field: "${spec}"`);
  return out;
}

export function parseCron(expr: string): ParsedCron {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) {
    throw new Error(`Cron expression must have 5 fields, got ${parts.length}: "${expr}"`);
  }
  const dowRaw = parseField(parts[4], 0, 7);
  // Normalize Sunday: 7 → 0.
  const dow = new Set<number>();
  for (const v of dowRaw) dow.add(v === 7 ? 0 : v);
  return {
    minute: parseField(parts[0], 0, 59),
    hour: parseField(parts[1], 0, 23),
    dom: parseField(parts[2], 1, 31),
    month: parseField(parts[3], 1, 12),
    dow,
    domRestricted: parts[2] !== "*",
    dowRestricted: parts[4] !== "*",
  };
}

function dayMatches(d: number, w: number, p: ParsedCron): boolean {
  if (p.domRestricted && p.dowRestricted) return p.dom.has(d) || p.dow.has(w);
  if (p.domRestricted) return p.dom.has(d);
  if (p.dowRestricted) return p.dow.has(w);
  return true;
}

/**
 * Returns the next time (epoch ms, UTC) the cron expression would fire after
 * `fromMs`. Throws if no match exists within one year.
 */
export function nextCronRun(expr: string, fromMs: number = Date.now()): number {
  const p = parseCron(expr);
  // Round up to the next whole minute strictly after `fromMs`.
  let t = Math.floor(fromMs / 60_000) * 60_000 + 60_000;
  // Worst case: walk every minute for a year. In practice converges in a few
  // iterations because we jump-skip non-matching months/days/hours.
  const cap = 366 * 24 * 60;
  for (let i = 0; i < cap; i++) {
    const d = new Date(t);
    const Y = d.getUTCFullYear();
    const Mo = d.getUTCMonth();
    const M = Mo + 1;
    const D = d.getUTCDate();
    const W = d.getUTCDay();
    const h = d.getUTCHours();
    const m = d.getUTCMinutes();

    if (!p.month.has(M)) {
      const nextMonth = Mo === 11 ? 0 : Mo + 1;
      const nextYear = Mo === 11 ? Y + 1 : Y;
      t = Date.UTC(nextYear, nextMonth, 1);
      continue;
    }
    if (!dayMatches(D, W, p)) {
      t = Date.UTC(Y, Mo, D + 1);
      continue;
    }
    if (!p.hour.has(h)) {
      t = Date.UTC(Y, Mo, D, h + 1);
      continue;
    }
    if (!p.minute.has(m)) {
      t += 60_000;
      continue;
    }
    return t;
  }
  throw new Error(`No cron match within 1 year for "${expr}"`);
}
