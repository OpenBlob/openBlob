import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  plugins: [tailwindcss(), reactRouter(), tsconfigPaths()],
  server: {
    port: 3000,
  },
  // Intentionally no `ssr.noExternal` here. Inlining wagmi/viem into the SSR
  // bundle pulled in 3+ duplicated copies of `ox` (a viem dependency) via
  // wagmi's deeply nested connector ecosystem (@base-org/account, porto,
  // @coinbase/wallet-sdk, …), which OOM'd Rollup. Externalizing them lets
  // Deno resolve them at runtime via the local node_modules (see
  // `nodeModulesDir: "auto"` in deno.json).
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
