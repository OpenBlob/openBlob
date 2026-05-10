import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  plugins: [tailwindcss(), reactRouter(), tsconfigPaths()],
  server: {
    port: 3000,
  },
  ssr: {
    // Bundle these for SSR so Deno doesn't try to resolve them as bare specifiers.
    noExternal: ["wagmi", "viem", "@tanstack/react-query"],
  },
  ...denoWorkaround(),
});

// Under Deno, `react-dom/server` resolves to the Node build which uses
// CommonJS `require()` internals that Vite's SSR module runner cannot
// evaluate at dev time. Pin every consumer to the Web-Streams ("edge") build
// instead — it's pure ESM and works in Node, Deno, Bun, and edge runtimes.
function denoWorkaround() {
  const isDeno = typeof globalThis !== "undefined" && "Deno" in globalThis;
  if (!isDeno) return undefined;
  return {
    resolve: {
      alias: {
        "react-dom/server": "react-dom/server.edge",
      },
    },
  };
}
