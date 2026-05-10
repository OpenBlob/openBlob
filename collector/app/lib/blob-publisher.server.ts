/**
 * Bundler-side blob publishing.
 *
 * Implements the encoder side of openBlob/specs/spec.md:
 *   §3   field-element packing (via viem.toBlobs)
 *   §3.0 cross-blob logical stream `S`
 *   §4   RLP-framed entries (via viem.toRlp)
 *
 * KZG commitments + proofs are computed via `kzg-wasm` (pure WASM, so this
 * works under Deno Deploy where `c-kzg`'s native bindings would fail), and
 * a type-3 transaction is sent to Sepolia from the account configured by
 * `BUNDLER_PRIVATE_KEY`.
 *
 * Returned `used` is the slice of microblobs that fit within the EIP-4844
 * 6-blob-per-tx cap; overflow is left pending for the next cron tick.
 */

import { loadKZG } from "kzg-wasm";
import {
  type Address,
  bytesToHex,
  createWalletClient,
  type Hex,
  hexToBytes,
  http,
  type Kzg,
  parseGwei,
  toBlobs,
  toBytes,
  toRlp,
  type WalletClient,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { sepolia } from "viem/chains";

import type { Microblob } from "./kv.server.ts";

const DEFAULT_RPC_URL = "https://sepolia.drpc.org";
/** EIP-4844 caps a transaction at this many blobs. */
const MAX_BLOBS_PER_TX = 6;
/** Tip we tag onto the blob gas market. 30 gwei is generous on Sepolia. */
const MAX_FEE_PER_BLOB_GAS = parseGwei("30");

export type Publisher = {
  publishMicroblobs(microblobs: Microblob[]): Promise<{ txHash: Hex; used: Microblob[] }>;
};

/**
 * §4 framing: encode the user-submitted payloads of a batch of microblobs
 * as a canonical RLP list of byte strings. Addresses, signatures, hashes
 * and timestamps stay in KV (§5 sidecar).
 */
export function encodeMicroblobsRlp(microblobs: Microblob[]): Hex {
  return toRlp(microblobs.map((m) => toBytes(m.payload)));
}

let kzgPromise: Promise<Kzg> | null = null;

function getKzg(): Promise<Kzg> {
  if (kzgPromise) return kzgPromise;
  kzgPromise = (async () => {
    const wasmKzg = await loadKZG();
    // kzg-wasm exchanges hex strings; viem's Kzg interface speaks ByteArray.
    return {
      blobToKzgCommitment(blob: Uint8Array): Uint8Array {
        return hexToBytes(wasmKzg.blobToKZGCommitment(bytesToHex(blob)) as Hex);
      },
      computeBlobKzgProof(blob: Uint8Array, commitment: Uint8Array): Uint8Array {
        return hexToBytes(wasmKzg.computeBlobKZGProof(bytesToHex(blob), bytesToHex(commitment)) as Hex);
      },
    } satisfies Kzg;
  })();
  return kzgPromise;
}

type Env = {
  privateKey: Hex;
  rpcUrl: string;
  to?: Address;
};

function readEnv(): Env | null {
  const deno = (globalThis as { Deno?: typeof Deno }).Deno;
  const get = (name: string) => deno?.env.get(name)?.trim() || undefined;

  const privateKey = get("BUNDLER_PRIVATE_KEY");
  if (!privateKey) return null;
  if (!/^0x[0-9a-fA-F]{64}$/.test(privateKey)) {
    throw new Error("BUNDLER_PRIVATE_KEY must be a 0x-prefixed 32-byte hex string");
  }

  const to = get("BUNDLER_TO");
  if (to && !/^0x[0-9a-fA-F]{40}$/.test(to)) {
    throw new Error("BUNDLER_TO must be a 0x-prefixed 20-byte address");
  }

  return {
    privateKey: privateKey as Hex,
    rpcUrl: get("BUNDLER_RPC_URL") ?? DEFAULT_RPC_URL,
    to: to as Address | undefined,
  };
}

let cachedPublisher: Publisher | null = null;
let cachedFromKey: Hex | null = null;

export function getPublisher(): Publisher | null {
  const env = readEnv();
  if (!env) return null;

  // Re-build the wallet client if the key changes (mostly for HMR).
  if (cachedPublisher && cachedFromKey === env.privateKey) return cachedPublisher;

  const account = privateKeyToAccount(env.privateKey);
  const client = createWalletClient({
    account,
    chain: sepolia,
    transport: http(env.rpcUrl),
  });
  const to: Address = env.to ?? account.address;

  cachedFromKey = env.privateKey;
  cachedPublisher = {
    async publishMicroblobs(microblobs) {
      if (microblobs.length === 0) {
        throw new Error("publishMicroblobs called with empty list");
      }

      // Fit the batch into ≤ 6 blobs by trimming the tail. Re-encode each
      // shrink because RLP framing changes with the list contents.
      let used = microblobs;
      let blobs = toBlobs({ data: encodeMicroblobsRlp(used) });
      while (blobs.length > MAX_BLOBS_PER_TX && used.length > 1) {
        used = used.slice(0, used.length - 1);
        blobs = toBlobs({ data: encodeMicroblobsRlp(used) });
      }
      if (blobs.length > MAX_BLOBS_PER_TX) {
        throw new Error(`single microblob payload too large: needs ${blobs.length} blobs (max ${MAX_BLOBS_PER_TX})`);
      }

      const kzg = await getKzg();
      const txHash = await sendBlobTx(client, { blobs, kzg, to });
      return { txHash, used };
    },
  };
  return cachedPublisher;
}

async function sendBlobTx(
  client: WalletClient,
  params: { blobs: readonly Hex[]; kzg: Kzg; to: Address },
): Promise<Hex> {
  // Cast through unknown: viem's overloaded `sendTransaction` types disagree
  // with our pre-resolved (account, chain) shape, but the runtime call is
  // correct.
  return await (
    client.sendTransaction as unknown as (args: {
      blobs: readonly Hex[];
      kzg: Kzg;
      to: Address;
      maxFeePerBlobGas: bigint;
    }) => Promise<Hex>
  )({
    blobs: params.blobs,
    kzg: params.kzg,
    to: params.to,
    maxFeePerBlobGas: MAX_FEE_PER_BLOB_GAS,
  });
}
