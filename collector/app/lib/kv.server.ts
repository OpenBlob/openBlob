import type { Address, Hex } from "viem";

/**
 * Shape of a microblob record stored in Deno KV.
 *
 * Keys live under:
 *   ["microblobs", "by_id", id]      → Microblob
 *   ["microblobs", "by_status", "pending", createdAt, id] → id (secondary index)
 *   ["microblobs", "by_status", "bundled", createdAt, id] → id (secondary index)
 *   ["microblobs", "by_tx", bundleTxHash, createdAt, id]  → id (secondary index)
 */
export type Microblob = {
  id: string;
  address: Address;
  payload: string;
  signature: Hex;
  /** keccak256 of the EIP-191 personal_sign digest of the payload. */
  hash: Hex;
  status: "pending" | "bundled";
  createdAt: number;
  bundledAt?: number;
  /** Set when a bundle is published; references the on-chain blob tx hash. */
  bundleTxHash?: Hex;
};

let cachedKv: Deno.Kv | null = null;

export async function getKv(): Promise<Deno.Kv> {
  if (cachedKv) return cachedKv;
  const path = (globalThis as { Deno?: typeof Deno }).Deno?.env.get("DENO_KV_PATH");
  cachedKv = await Deno.openKv(path || undefined);
  return cachedKv;
}

const ID_KEY = (id: string) => ["microblobs", "by_id", id] as const;
const PENDING_KEY = (createdAt: number, id: string) => ["microblobs", "by_status", "pending", createdAt, id] as const;
const BUNDLED_KEY = (createdAt: number, id: string) => ["microblobs", "by_status", "bundled", createdAt, id] as const;
const BY_TX_KEY = (txHash: Hex, createdAt: number, id: string) =>
  ["microblobs", "by_tx", txHash, createdAt, id] as const;

/**
 * Tx hashes indexed via {@link BY_TX_KEY} are normalized to lowercase so that
 * lookups by raw user input (which may be 0xABC… or 0xabc…) hit the same key
 * the writer used.
 */
function normalizeTxHash(txHash: Hex): Hex {
  return txHash.toLowerCase() as Hex;
}

export async function putMicroblob(microblob: Microblob): Promise<void> {
  const kv = await getKv();
  const indexKey =
    microblob.status === "pending"
      ? PENDING_KEY(microblob.createdAt, microblob.id)
      : BUNDLED_KEY(microblob.createdAt, microblob.id);
  const res = await kv.atomic().set(ID_KEY(microblob.id), microblob).set(indexKey, microblob.id).commit();
  if (!res.ok) throw new Error("kv: failed to write microblob");
}

export async function getMicroblob(id: string): Promise<Microblob | null> {
  const kv = await getKv();
  const entry = await kv.get<Microblob>(ID_KEY(id));
  return entry.value;
}

export async function listMicroblobs(opts?: { status?: "pending" | "bundled"; limit?: number }): Promise<Microblob[]> {
  const kv = await getKv();
  const status = opts?.status ?? "pending";
  const limit = opts?.limit ?? 50;
  const prefix = ["microblobs", "by_status", status];
  const ids: string[] = [];
  for await (const entry of kv.list<string>({ prefix }, { limit, reverse: true })) {
    ids.push(entry.value);
  }
  if (ids.length === 0) return [];
  const items = await kv.getMany<Microblob[]>(ids.map((id) => ID_KEY(id)));
  return items.map((entry) => entry.value).filter((value): value is Microblob => value !== null);
}

export async function markBundled(ids: string[], bundleTxHash: Hex): Promise<number> {
  const kv = await getKv();
  // Normalize once so both the stored field and the by_tx index agree on case.
  const normalizedTxHash = normalizeTxHash(bundleTxHash);
  let updated = 0;
  for (const id of ids) {
    const entry = await kv.get<Microblob>(ID_KEY(id));
    if (!entry.value || entry.value.status !== "pending") continue;
    const bundledAt = Date.now();
    const next: Microblob = {
      ...entry.value,
      status: "bundled",
      bundledAt,
      bundleTxHash: normalizedTxHash,
    };
    const res = await kv
      .atomic()
      .check(entry)
      .set(ID_KEY(id), next)
      .delete(PENDING_KEY(entry.value.createdAt, id))
      .set(BUNDLED_KEY(bundledAt, id), id)
      .set(BY_TX_KEY(normalizedTxHash, entry.value.createdAt, id), id)
      .commit();
    if (res.ok) updated++;
  }
  return updated;
}

/**
 * Look up every microblob bundled into a given EIP-4844 blob transaction.
 *
 * Returns records in `createdAt` order (oldest first) so consumers can
 * reproduce the spec §4 RLP entry order produced by the bundler — that
 * encoder iterates the pending list in `createdAt` order too.
 *
 * Note: only txs produced by this collector are indexed. Microblobs bundled
 * before the `by_tx` index existed will be missing here even though their
 * `bundleTxHash` field is populated.
 */
export async function listMicroblobsByTx(txHash: Hex): Promise<Microblob[]> {
  const kv = await getKv();
  const prefix = ["microblobs", "by_tx", normalizeTxHash(txHash)];
  const ids: string[] = [];
  for await (const entry of kv.list<string>({ prefix })) {
    ids.push(entry.value);
  }
  if (ids.length === 0) return [];
  const items = await kv.getMany<Microblob[]>(ids.map((id) => ID_KEY(id)));
  return items
    .map((entry) => entry.value)
    .filter((value): value is Microblob => value !== null)
    .sort((a, b) => a.createdAt - b.createdAt);
}
