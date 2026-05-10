import { isbot } from "isbot";
// Force the Web-Streams ("edge") build of `react-dom/server` so this entry runs
// under Deno + Vite's SSR module runner. The default `react-dom/server`
// resolves to the Node build, which uses `require()` internals that Vite's
// dev-mode module runner cannot evaluate.
import { renderToReadableStream } from "react-dom/server.edge";
import { type AppLoadContext, type EntryContext, ServerRouter } from "react-router";

// Register the bundling cron at SSR-entry load time, but only in dev:
// `deno task dev` never loads `server.ts`, so this is the only place where
// we can wire up `Deno.cron` for local development. In production builds
// Vite replaces `import.meta.env.DEV` with `false` and tree-shakes the
// dynamic import out, so request isolates never pay to parse the bundler
// module graph (which transitively pulls in viem + kzg-wasm).
if (import.meta.env.DEV) {
  void import("~/lib/bundler-cron.server").then((m) => m.registerBundlerCron());
}

export const streamTimeout = 5_000;

export default async function handleRequest(
  request: Request,
  responseStatusCode: number,
  responseHeaders: Headers,
  routerContext: EntryContext,
  _loadContext: AppLoadContext,
) {
  let didError = false;
  const body = await renderToReadableStream(<ServerRouter context={routerContext} url={request.url} />, {
    signal: AbortSignal.timeout(streamTimeout),
    onError(error: unknown) {
      didError = true;
      console.error(error);
    },
  });

  const userAgent = request.headers.get("user-agent");
  if (userAgent && isbot(userAgent)) {
    await body.allReady;
  }

  responseHeaders.set("Content-Type", "text/html");
  return new Response(body, {
    headers: responseHeaders,
    status: didError ? 500 : responseStatusCode,
  });
}
