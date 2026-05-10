import { isbot } from "isbot";
import { renderToReadableStream } from "react-dom/server";
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

export default async function handleRequest(
  request: Request,
  responseStatusCode: number,
  responseHeaders: Headers,
  routerContext: EntryContext,
  _loadContext: AppLoadContext,
) {
  let shellRendered = false;

  const body = await renderToReadableStream(<ServerRouter context={routerContext} url={request.url} />, {
    onError(error: unknown) {
      responseStatusCode = 500;
      // Only log post-shell streaming errors. Errors thrown during initial
      // shell rendering reject the `renderToReadableStream` promise and are
      // surfaced through React Router's error boundary plumbing.
      if (shellRendered) {
        console.error(error);
      }
    },
  });
  shellRendered = true;

  // Bots and SPA Mode renders need the full HTML up front (no progressive
  // streaming), so wait for every Suspense boundary to settle before
  // sending the response.
  const userAgent = request.headers.get("user-agent");
  if ((userAgent && isbot(userAgent)) || routerContext.isSpaMode) {
    await body.allReady;
  }

  responseHeaders.set("Content-Type", "text/html");
  return new Response(body, {
    headers: responseHeaders,
    status: responseStatusCode,
  });
}
