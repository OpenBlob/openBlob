/**
 * OpenBlob production Deno server.
 *
 * - Serves hashed Vite assets from `build/client/assets/` via `serveDir`,
 *   which sets `ETag` + `Last-Modified` and answers conditional requests
 *   with `304 Not Modified` (so navigations don't re-stream every JS chunk).
 * - Delegates everything else to the React Router request handler from
 *   `build/server/index.js`.
 * - Registers a `Deno.cron` that bundles pending microblobs in-process
 *   (no HTTP roundtrip back to ourselves).
 *
 * Run with:
 *   deno task build && deno task start
 *
 * For development, run `deno task dev` (which uses `react-router dev`).
 *
 * This mirrors the official `remix-run/react-router-templates/deno`
 * template, with two project-specific tweaks preserved:
 *   1. The SSR build is imported lazily (`createRequestHandler` is given
 *      a thunk) so the Deno Deploy cron isolate — which evaluates this
 *      file just to register the cron callback — never pays to parse the
 *      entire React Router server bundle on cold start.
 *   2. `registerBundlerCron()` is called at module load so the same
 *      isolate that boots the HTTP server (or the cron isolate on Deno
 *      Deploy) wires up the bundling job.
 */

import { serveDir } from "@std/http/file-server";
import { createRequestHandler, type ServerBuild } from "react-router";

import { registerBundlerCron } from "./app/lib/bundler-cron.server.ts";

// `new URL(..., import.meta.url)` keeps the build paths anchored to this
// file regardless of cwd, which matters on Deno Deploy.
const buildServerUrl = new URL("./build/server/index.js", import.meta.url).href;
const buildClientDir = new URL("./build/client/", import.meta.url).pathname;

const handler = createRequestHandler(
  () => import(buildServerUrl) as Promise<ServerBuild>,
  "production",
);

const ONE_YEAR = 60 * 60 * 24 * 365;

Deno.serve(async (request) => {
  const pathname = new URL(request.url).pathname;

  // Hashed Vite assets — long-lived and immutable. `serveDir` handles
  // ETag, Last-Modified, conditional requests (304), and Range.
  if (pathname.startsWith("/assets/")) {
    return serveDir(request, {
      fsRoot: `${buildClientDir}assets`,
      urlRoot: "assets",
      headers: [`Cache-Control: public, max-age=${ONE_YEAR}, immutable`],
      quiet: true,
    });
  }

  return handler(request);
});

// Bundle pending microblobs in-process. On Deno Deploy this runs in a
// dedicated cron isolate, but it still has the same Deno KV view as the
// request isolates, so calling KV directly is sufficient — no need to
// fetch back into ourselves over HTTP.
registerBundlerCron();
