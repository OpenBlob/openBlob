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

import type { Hex } from "viem";

import { listMicroblobs, markBundled } from "./kv.server.ts";

const DEFAULT_SCHEDULE = "* * * * *";
const CRON_NAME = "openblob-process";
const REGISTERED_FLAG = "__openblobBundlerCronRegistered__";

async function processPending(): Promise<void> {
  const pending = await listMicroblobs({ status: "pending", limit: 100 });
  if (pending.length === 0) {
    console.log("[cron] no pending microblobs");
    return;
  }

  // TODO: assemble an EIP-4844 blob transaction from `pending` and broadcast
  // it. For now we mark them bundled with a placeholder tx hash.
  const placeholderTxHash = `0x${"0".repeat(64)}` as Hex;
  const bundled = await markBundled(
    pending.map((m) => m.id),
    placeholderTxHash,
  );
  console.log(`[cron] bundled ${bundled} microblob(s) tx=${placeholderTxHash}`);
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

  const schedule = deno.env.get("CRON_SCHEDULE")?.trim() || DEFAULT_SCHEDULE;
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
