# openblob

OpenBlob collects signed microblobs from connected wallets, stores them in
[Deno KV](https://docs.deno.com/deploy/kv/manual/), and a server-side
[`Deno.cron`](https://docs.deno.com/deploy/kv/manual/cron/) bundles pending
microblobs into Ethereum [EIP-4844 blob transactions](https://eips.ethereum.org/EIPS/eip-4844)
on a configurable schedule (default `0 0 * * *` — once per day at 00:00 UTC,
override with `CRON_SCHEDULE`).

## Stack

- [React Router v7](https://reactrouter.com/) in framework (meta-framework) mode with SSR.
- [Vite 7](https://vitejs.dev/) for bundling, run via Deno.
- [Tailwind v4](https://tailwindcss.com/) + [shadcn/ui](https://ui.shadcn.com/) (`new-york`, `neutral`).
- [wagmi 2](https://wagmi.sh/) + [viem 2](https://viem.sh/) for wallet + Ethereum.
- [Deno 2.x](https://deno.com/) as runtime, package manager, and HTTP server.
- [Deno KV](https://docs.deno.com/deploy/kv/manual/) for storage.
- [Deno Cron](https://docs.deno.com/deploy/kv/manual/cron/) for the periodic bundling job.
- [Biome](https://biomejs.dev/) for linting/formatting.

## Layout

```
openblob/
├── deno.json                   # Deno tasks, unstable flags, npm bridge
├── package.json                # npm deps (Vite + React Router + ecosystem)
├── react-router.config.ts      # framework config (ssr: true)
├── vite.config.ts              # Vite + React Router + Tailwind plugins
├── server.ts                   # Deno production server + Deno.cron bundler
└── app/
    ├── root.tsx                # Document layout, providers, error boundary
    ├── entry.client.tsx        # Hydration entry
    ├── entry.server.tsx        # SSR entry
    ├── routes.ts               # flatRoutes() configuration
    ├── routes/
    │   ├── _index/route.tsx              # Landing page (sign + submit)
    │   ├── api.microblobs.ts             # GET list, POST submit signed microblob
    │   └── api.microblobs.tx.$txHash.ts  # GET microblobs bundled into a blob tx
    ├── components/
    │   ├── ui/                 # shadcn components (button, card, input, textarea)
    │   └── connect-button.tsx  # wagmi connect / disconnect
    ├── providers/
    │   └── web3-provider.tsx   # WagmiProvider + QueryClientProvider
    └── lib/
        ├── kv.server.ts        # Deno KV helpers (server-only)
        ├── utils.ts            # cn() and friends
        └── wagmi.config.ts     # wagmi config (mainnet + sepolia)
```

## Getting started

Install dependencies (Deno materializes `node_modules/` from `package.json`):

```bash
deno install
```

Generate React Router route types and start the dev server:

```bash
deno task dev
```

Open [http://localhost:3000](http://localhost:3000).

> Note: `deno task dev` runs `react-router dev`, which starts its own Vite dev
> server. The bundling cron is registered from `app/entry.server.tsx` so it
> fires in dev too (after the SSR entry has been loaded by the first request).

## Production

```bash
deno task build   # react-router build → build/server + build/client
deno task start   # runs server.ts (Deno HTTP + Deno.cron)
```

`server.ts` wires:

1. Static asset serving from `build/client/`.
2. The React Router request handler from `build/server/index.js`.
3. `Deno.cron("openblob-process", "0 0 * * *", …)` which calls the bundling
   logic in-process (reads pending microblobs from Deno KV, marks them
   bundled). No HTTP roundtrip back into the server. The schedule is
   overridable via `CRON_SCHEDULE`.

The local production server binds to Deno's default port (`8000`). On Deno
Deploy the platform manages the listener, so the chosen port is irrelevant.

## HTTP API

| Method | Path                            | Description                                               |
| ------ | ------------------------------- | --------------------------------------------------------- |
| `GET`  | `/api/microblobs`               | List the most recent pending microblobs.                  |
| `POST` | `/api/microblobs`               | Submit `{ address, payload, signature }`. Verifies EIP-191. |
| `GET`  | `/api/microblobs/tx/:txHash`    | List every microblob this collector bundled into the given EIP-4844 blob tx. |

The `tx/:txHash` endpoint returns the full sidecar — payload, signer, signature,
and per-entry message hash — for each microblob, in the same `createdAt` order
the bundler used when building the spec §4 RLP list. Signatures live in KV
(spec §5: out-of-band) so this endpoint only knows about txs produced by this
collector; foreign blob txs return an empty list.

```bash
curl -s http://localhost:3000/api/microblobs/tx/0xabc...def | jq
# {
#   "txHash": "0xabc...def",
#   "count": 2,
#   "microblobs": [
#     { "id": "...", "address": "0x...", "payload": "hello blob",
#       "signature": "0x...", "hash": "0x...", "status": "bundled",
#       "createdAt": 1736500000000, "bundledAt": 1736500060000,
#       "bundleTxHash": "0xabc...def" },
#     ...
#   ]
# }
```

## Environment variables

See `.env.example`. Notable knobs:

- `CRON_SCHEDULE` — overrides the default `0 0 * * *` (once per day). Set to e.g. `* * * * *` to bundle every minute or `*/5 * * * *` for every 5 minutes, at the cost of more cron-isolate cold starts on Deno Deploy.
- `DENO_KV_PATH` — optional path for the Deno KV file (defaults to in-memory in dev, persistent in Deno Deploy).

## Adding shadcn components

```bash
deno task shadcn add dropdown-menu
```

The CLI is configured via `components.json` (alias `~/components/ui`,
Tailwind CSS at `app/app.css`).
