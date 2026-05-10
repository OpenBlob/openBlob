import { isRouteErrorResponse, Links, Meta, Outlet, Scripts, ScrollRestoration } from "react-router";

import type { Route } from "./+types/root";
import "./app.css";
import { Web3Provider } from "~/providers/web3-provider";

export const links: Route.LinksFunction = () => [];

export function meta(): Route.MetaDescriptors {
  return [
    { title: "OpenBlob — Collect microblobs, post EIP-4844 blobs" },
    {
      name: "description",
      content:
        "OpenBlob aggregates signed microblobs from connected wallets and bundles them into Ethereum blob transactions.",
    },
  ];
}

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <Meta />
        <Links />
      </head>
      <body className="min-h-screen bg-background text-foreground antialiased">
        {children}
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  );
}

export default function App() {
  return (
    <Web3Provider>
      <Outlet />
    </Web3Provider>
  );
}

export function HydrateFallback() {
  return (
    <main className="grid min-h-screen place-items-center p-8">
      <p className="text-muted-foreground text-sm">Loading OpenBlob…</p>
    </main>
  );
}

export function ErrorBoundary({ error }: Route.ErrorBoundaryProps) {
  let message = "Oops!";
  let details = "An unexpected error occurred.";
  let stack: string | undefined;

  if (isRouteErrorResponse(error)) {
    message = error.status === 404 ? "404" : "Error";
    details = error.status === 404 ? "The requested page could not be found." : error.statusText || details;
  } else if (import.meta.env.DEV && error instanceof Error) {
    details = error.message;
    stack = error.stack;
  }

  return (
    <main className="container mx-auto p-4 pt-16">
      <h1 className="font-bold text-3xl">{message}</h1>
      <p className="mt-2 text-muted-foreground">{details}</p>
      {stack && (
        <pre className="mt-4 w-full overflow-x-auto rounded-md border bg-card p-4 text-xs">
          <code>{stack}</code>
        </pre>
      )}
    </main>
  );
}
