/**
 * GET /api/microblobs/tx/:txHash
 *
 * Returns every microblob this collector bundled into the given EIP-4844 blob
 * transaction, including the on-blob payload (spec §4) plus the §5 sidecar
 * fields (signer address, signature, message hash) which live in KV — never
 * inside the blob itself.
 *
 * Lookup is exclusively by the `by_tx` KV index written in `markBundled`.
 * Foreign txs (i.e. blob txs not produced by this collector) and any tx that
 * predates the index will return an empty list because the signatures in §5
 * are out-of-band and cannot be recovered from chain data alone.
 */

import type { Hex } from "viem";

import { listMicroblobsByTx } from "~/lib/kv.server";
import type { Route } from "./+types/api.microblobs.tx.$txHash";

const TX_HASH_RE = /^0x[0-9a-fA-F]{64}$/;

export async function loader({ params }: Route.LoaderArgs) {
  const { txHash } = params;
  if (!TX_HASH_RE.test(txHash)) {
    return Response.json({ error: "Invalid txHash: expected 0x-prefixed 32-byte hex string" }, { status: 400 });
  }

  const normalized = txHash.toLowerCase() as Hex;
  const microblobs = await listMicroblobsByTx(normalized);
  return Response.json({ txHash: normalized, count: microblobs.length, microblobs });
}
