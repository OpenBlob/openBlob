import { useEffect, useState } from "react";
import { useRevalidator } from "react-router";
import { useAccount, useSignMessage } from "wagmi";

import { ConnectButton } from "~/components/connect-button";
import { NextBundleTimer } from "~/components/next-bundle-timer";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "~/components/ui/card";
import { Textarea } from "~/components/ui/textarea";
import { nextCronRun } from "~/lib/cron";
import { listMicroblobs, type Microblob } from "~/lib/kv.server";

import type { Route } from "./+types/route";

const DEFAULT_CRON_SCHEDULE = "* * * * *";

function getCronSchedule(): string {
  // Mirror the resolution server.ts uses, but tolerate environments where
  // `Deno` isn't available (e.g. type-checking with tsc).
  const denoEnv = (globalThis as { Deno?: { env: { get(name: string): string | undefined } } }).Deno?.env;
  const fromEnv = denoEnv?.get("CRON_SCHEDULE");
  return fromEnv && fromEnv.trim().length > 0 ? fromEnv : DEFAULT_CRON_SCHEDULE;
}

export async function loader(_args: Route.LoaderArgs) {
  // Read straight from KV instead of self-fetching `/api/microblobs`. The
  // self-fetch was both slower (extra HTTP roundtrip back into ourselves)
  // and fragile in production: if anything in front of the server returns
  // non-JSON for the API path (an error page, an auth gate, a misrouted
  // request, etc.), `res.json()` here would throw `Unexpected non-whitespace
  // character after JSON`.
  const microblobs = await listMicroblobs({ status: "pending", limit: 50 });
  const cronSchedule = getCronSchedule();
  let nextRunAt: number;
  try {
    nextRunAt = nextCronRun(cronSchedule);
  } catch {
    nextRunAt = nextCronRun(DEFAULT_CRON_SCHEDULE);
  }
  return { microblobs, cronSchedule, nextRunAt };
}

export default function Home({ loaderData }: Route.ComponentProps) {
  const [payload, setPayload] = useState("hello blob");
  const [submitting, setSubmitting] = useState(false);
  const [microblobs, setMicroblobs] = useState<Microblob[]>(loaderData.microblobs);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const { address, isConnected } = useAccount();
  const { signMessageAsync, isPending: isSigning } = useSignMessage();
  const revalidator = useRevalidator();

  useEffect(() => {
    setMicroblobs(loaderData.microblobs);
  }, [loaderData.microblobs]);

  // After the cron deadline elapses, give the in-process job a moment to
  // mark its batch as bundled in KV, then revalidate so the pending list
  // drops the items it just shipped.
  function handleBundleElapsed() {
    window.setTimeout(() => {
      revalidator.revalidate();
    }, 2000);
  }

  async function handleSubmit() {
    if (!address) return;
    setSubmitError(null);
    setSubmitting(true);
    try {
      const signature = await signMessageAsync({ message: payload });
      const res = await fetch("/api/microblobs", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ address, payload, signature }),
      });
      if (!res.ok) {
        const body = (await res.json().catch(() => null)) as { error?: string } | null;
        throw new Error(body?.error ?? `Server returned ${res.status}`);
      }
      const created = (await res.json()) as { microblob: Microblob };
      setMicroblobs((prev) => [created.microblob, ...prev]);
    } catch (err) {
      setSubmitError(err instanceof Error ? err.message : "Failed to submit microblob");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen bg-background">
      <header className="sticky top-0 z-10 border-b bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-5xl items-center justify-between px-6 py-4">
          <div className="flex items-center gap-2">
            <div className="size-7 rounded-md bg-foreground" />
            <span className="font-semibold tracking-tight">OpenBlob</span>
          </div>
          <ConnectButton />
        </div>
      </header>

      <main className="mx-auto flex max-w-5xl flex-col gap-8 px-6 py-12">
        <section className="flex flex-col gap-3">
          <h1 className="font-bold text-4xl tracking-tight">Collect microblobs, post EIP-4844 blobs.</h1>
          <p className="max-w-2xl text-muted-foreground">
            OpenBlob aggregates signed payloads from connected wallets, persists them in Deno KV, and a server-side cron
            bundles pending microblobs into Ethereum blob transactions every minute.
          </p>
          <NextBundleTimer
            schedule={loaderData.cronSchedule}
            initialNextRunAt={loaderData.nextRunAt}
            onElapsed={handleBundleElapsed}
          />
        </section>

        <section className="grid gap-6 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Submit a microblob</CardTitle>
              <CardDescription>
                Sign a payload with your wallet to contribute it to the next blob bundle.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <Textarea
                value={payload}
                onChange={(e) => setPayload(e.target.value)}
                placeholder="Anything you want to publish as part of a blob…"
                rows={5}
                disabled={!isConnected}
              />
              <div className="flex items-center justify-between">
                <p className="text-muted-foreground text-xs">
                  {isConnected ? "Signed and stored in Deno KV." : "Connect a wallet to enable signing."}
                </p>
                <Button onClick={handleSubmit} disabled={!isConnected || !payload.trim() || submitting || isSigning}>
                  {isSigning ? "Sign in wallet…" : submitting ? "Submitting…" : "Sign & submit"}
                </Button>
              </div>
              {submitError && <p className="text-destructive text-xs">{submitError}</p>}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Pending microblobs</CardTitle>
              <CardDescription>The freshest entries waiting to be bundled.</CardDescription>
            </CardHeader>
            <CardContent>
              {microblobs.length === 0 ? (
                <p className="text-muted-foreground text-sm">No microblobs yet — be the first to submit one.</p>
              ) : (
                <ul className="flex flex-col gap-3">
                  {microblobs.map((m) => (
                    <li key={m.id} className="rounded-md border bg-card/50 p-3">
                      <div className="flex items-center justify-between">
                        <span className="font-mono text-xs">{m.address}</span>
                        <span className="text-muted-foreground text-xs">
                          {new Date(m.createdAt).toLocaleTimeString()}
                        </span>
                      </div>
                      <p className="mt-1 truncate text-sm">{m.payload}</p>
                      <p className="mt-1 truncate font-mono text-muted-foreground text-xs">hash {m.hash}</p>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>
        </section>
      </main>
    </div>
  );
}
