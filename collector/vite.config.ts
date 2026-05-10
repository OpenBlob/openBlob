import deno from "@deno/vite-plugin";
import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  plugins: [tailwindcss(), reactRouter(), deno(), tsconfigPaths()],
  server: {
    port: 3000,
  },
  // Match the official `remix-run/react-router-templates/deno` template:
  // ask Vite to pick the `"deno"` export condition for SSR resolution. This
  // is what lets us import `react-dom/server` directly (instead of pinning
  // `react-dom/server.edge` via a manual alias) — under Deno that condition
  // resolves to the Web-Streams build, which is the one Vite's SSR module
  // runner can evaluate.
  environments: {
    ssr: {
      build: {
        target: "ESNext",
      },
      resolve: {
        conditions: ["deno"],
        externalConditions: ["deno"],
      },
    },
  },
  // Intentionally no `ssr.noExternal` here. Inlining wagmi/viem into the SSR
  // bundle pulled in 3+ duplicated copies of `ox` (a viem dependency) via
  // wagmi's deeply nested connector ecosystem (@base-org/account, porto,
  // @coinbase/wallet-sdk, …), which OOM'd Rollup. Externalizing them lets
  // Deno resolve them at runtime via the local node_modules (see
  // `nodeModulesDir: "auto"` in deno.json).
});
