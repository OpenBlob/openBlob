import { Clock } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { nextCronRun } from "~/lib/cron";

type Props = {
  /** 5-field cron schedule (UTC), e.g. "* /5 * * * *". */
  schedule: string;
  /** Initial next-run timestamp (epoch ms) computed on the server. */
  initialNextRunAt: number;
  /**
   * Fired once each time the countdown crosses a scheduled run. Useful for
   * triggering a route revalidation so the UI catches up with whatever the
   * cron just did server-side. The callback is wrapped in a ref internally,
   * so it doesn't need to be referentially stable.
   */
  onElapsed?: () => void;
};

function formatRemaining(ms: number): string {
  if (ms <= 0) return "any moment now…";
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, "0")}m ${String(seconds).padStart(2, "0")}s`;
  if (minutes > 0) return `${minutes}m ${String(seconds).padStart(2, "0")}s`;
  return `${seconds}s`;
}

export function NextBundleTimer({ schedule, initialNextRunAt, onElapsed }: Props) {
  const [nextRunAt, setNextRunAt] = useState(initialNextRunAt);
  // `now` is intentionally null until the client mounts so SSR and the first
  // client render produce identical markup ("—") and React doesn't warn about
  // a hydration mismatch caused by clock drift between server and client.
  const [now, setNow] = useState<number | null>(null);

  const onElapsedRef = useRef(onElapsed);
  useEffect(() => {
    onElapsedRef.current = onElapsed;
  }, [onElapsed]);

  useEffect(() => {
    setNextRunAt(initialNextRunAt);
  }, [initialNextRunAt]);

  useEffect(() => {
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  // Once the countdown crosses the deadline, recompute the next run locally
  // so the timer keeps ticking without waiting on a server roundtrip, and
  // notify the parent so it can revalidate any data the cron just touched.
  useEffect(() => {
    if (now === null || now < nextRunAt) return;
    try {
      const upcoming = nextCronRun(schedule, now);
      if (upcoming !== nextRunAt) setNextRunAt(upcoming);
    } catch {
      // Schedule was invalid; leave the badge in "any moment now…" state.
    }
    onElapsedRef.current?.();
  }, [now, nextRunAt, schedule]);

  const display = now === null ? "—" : formatRemaining(Math.max(0, nextRunAt - now));
  const nextRunLabel = new Date(nextRunAt).toLocaleTimeString();

  return (
    <div
      className="inline-flex items-center gap-2 self-start rounded-full border bg-card/50 px-3 py-1 text-xs"
      title={`Next bundle scheduled for ${nextRunLabel} (${schedule} UTC)`}
    >
      <Clock className="size-3.5 text-muted-foreground" aria-hidden />
      <span className="text-muted-foreground">Next bundle in</span>
      <span className="font-mono tabular-nums">{display}</span>
    </div>
  );
}
