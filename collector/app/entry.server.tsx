import { isbot } from "isbot";
// Force the Web-Streams ("edge") build of `react-dom/server` so this entry runs
// under Deno + Vite's SSR module runner. The default `react-dom/server`
// resolves to the Node build, which uses `require()` internals that Vite's
// dev-mode module runner cannot evaluate.
import { renderToReadableStream } from "react-dom/server.edge";
import { type AppLoadContext, type EntryContext, ServerRouter } from "react-router";

import { registerBundlerCron } from "~/lib/bundler-cron.server";

// Register the bundling cron at SSR-entry load time. This is the only
// reliable hook in dev (`deno task dev`), where `server.ts` is never
// loaded — Vite hosts the React Router app directly. In production, the
// registrar's globalThis flag makes this a no-op (server.ts already
// registered the cron at process start).
registerBundlerCron();

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
