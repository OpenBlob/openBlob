/**
 * Registers the `Deno.cron` job that bundles pending microblobs.
 *
 * Both the production server (`server.ts`) and the React Router server entry
 * (`app/entry.server.tsx`) call `registerBundlerCron()` at module load. The
 * production server is the only thing loaded under `deno task start`; under
 * `deno task dev` only the React Router app is loaded by Vite, so importing
 * this from the server entry is what gives us a working cron in dev.
 *
 * `Deno.cron` rejects double-registration of the same handle name, so we use
 * a `globalThis` flag to make registration idempotent across HMR reloads and
 * across both registration sites.
 */

// `kv.server.ts` and `blob-publisher.server.ts` are imported lazily inside
// `processPending` so that:
//   - request isolates that import this module (transitively, via
//     `entry.server.tsx`) don't pay to parse `viem` + `kzg-wasm` on every
//     cold-start; and
//   - cron isolates only pull in the publisher (and viem) when there is
//     actually pending work to bundle.

/**
 * Default cron schedule used when `CRON_SCHEDULE` is unset. Once per day at
 * 00:00 UTC — the most conservative choice for Deno Deploy isolate cost.
 *
 * Exported so the SSR loader can use the same value as the actual cron
 * registration; previously the route had its own local default of
 * `"* * * * *"`, which made the "Next bundle in …" countdown wildly wrong
 * (it would tick down to ~0 every minute even though the cron only fires
 * once a day).
 */
export const DEFAULT_SCHEDULE = "0 0 * * *";
const CRON_NAME = "openblob-process";
const REGISTERED_FLAG = "__openblobBundlerCronRegistered__";

/**
 * Resolves the active cron schedule the same way `registerBundlerCron` does:
 * read `CRON_SCHEDULE` from the environment and fall back to
 * `DEFAULT_SCHEDULE` if it's unset or blank. Safe to call from contexts
 * where `Deno` may not exist (e.g. tsc type-checking) — returns the default
 * in that case.
 */
export function getCronSchedule(): string {
  const denoEnv = (globalThis as { Deno?: { env: { get(name: string): string | undefined } } }).Deno?.env;
  const fromEnv = denoEnv?.get("CRON_SCHEDULE")?.trim();
  return fromEnv && fromEnv.length > 0 ? fromEnv : DEFAULT_SCHEDULE;
}

async function processPending(): Promise<void> {
  const { listMicroblobs, markBundled } = await import("./kv.server.ts");
  const pending = await listMicroblobs({ status: "pending", limit: 100 });
  if (pending.length === 0) {
    console.log("[cron] no pending microblobs");
    return;
  }

  const { getPublisher } = await import("./blob-publisher.server.ts");
  const publisher = getPublisher();
  if (!publisher) {
    console.warn(`[cron] BUNDLER_PRIVATE_KEY unset — leaving ${pending.length} microblob(s) pending`);
    return;
  }

  const { txHash, used } = await publisher.publishMicroblobs(pending);
  const bundled = await markBundled(
    used.map((m) => m.id),
    txHash,
  );
  console.log(`[cron] published ${bundled} microblob(s) tx=${txHash}`);
}

export function registerBundlerCron(): void {
  const g = globalThis as Record<string, unknown>;
  if (g[REGISTERED_FLAG]) return;

  const deno = (globalThis as { Deno?: typeof Deno }).Deno;
  if (!deno || typeof deno.cron !== "function") {
    // Not running under Deno (e.g. tsc type-check); nothing to register.
    g[REGISTERED_FLAG] = true;
    return;
  }

  const schedule = getCronSchedule();
  try {
    deno.cron(CRON_NAME, schedule, async () => {
      try {
        await processPending();
      } catch (err) {
        console.error("[cron] processPending failed", err);
      }
    });
    g[REGISTERED_FLAG] = true;
    console.log(`[cron] registered "${CRON_NAME}" on schedule ${schedule}`);
  } catch (err) {
    // Most likely a duplicate-registration error from a parallel isolate or a
    // racy HMR reload — record it but don't crash the server.
    console.warn("[cron] registration skipped:", err);
    g[REGISTERED_FLAG] = true;
  }
}
