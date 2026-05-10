import { type Address, type Hex, hashMessage, isAddress, isHex, verifyMessage } from "viem";

import { listMicroblobs, putMicroblob } from "~/lib/kv.server";
import type { Route } from "./+types/api.microblobs";

export async function loader(_args: Route.LoaderArgs) {
  const microblobs = await listMicroblobs({ status: "pending", limit: 50 });
  return Response.json({ microblobs });
}

type SubmitBody = {
  address?: string;
  payload?: string;
  signature?: string;
};

export async function action({ request }: Route.ActionArgs) {
  if (request.method !== "POST") {
    return Response.json({ error: "Method not allowed" }, { status: 405 });
  }

  let body: SubmitBody;
  try {
    body = (await request.json()) as SubmitBody;
  } catch {
    return Response.json({ error: "Invalid JSON body" }, { status: 400 });
  }

  const { address, payload, signature } = body;

  if (typeof address !== "string" || !isAddress(address)) {
    return Response.json({ error: "Invalid address" }, { status: 400 });
  }
  if (typeof payload !== "string" || payload.length === 0) {
    return Response.json({ error: "payload is required" }, { status: 400 });
  }
  if (typeof signature !== "string" || !isHex(signature)) {
    return Response.json({ error: "Invalid signature" }, { status: 400 });
  }

  let valid = false;
  try {
    valid = await verifyMessage({
      address: address as Address,
      message: payload,
      signature: signature as Hex,
    });
  } catch {
    return Response.json({ error: "Malformed signature" }, { status: 400 });
  }
  if (!valid) {
    return Response.json({ error: "Signature does not match address" }, { status: 401 });
  }

  const microblob = {
    id: crypto.randomUUID(),
    address: address as Address,
    payload,
    signature: signature as Hex,
    hash: hashMessage(payload),
    status: "pending" as const,
    createdAt: Date.now(),
  };
  await putMicroblob(microblob);

  return Response.json({ microblob }, { status: 201 });
}
