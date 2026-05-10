import { useAccount, useConnect, useDisconnect } from "wagmi";

import { Button } from "~/components/ui/button";
import { formatAddress } from "~/lib/utils";

export function ConnectButton() {
  const { address, isConnected, status } = useAccount();
  const { connectors, connect, status: connectStatus, error } = useConnect();
  const { disconnect } = useDisconnect();

  if (isConnected && address) {
    return (
      <div className="flex items-center gap-2">
        <span className="rounded-md border bg-card px-2 py-1 font-mono text-xs">{formatAddress(address)}</span>
        <Button variant="outline" size="sm" onClick={() => disconnect()}>
          Disconnect
        </Button>
      </div>
    );
  }

  const injectedConnector = connectors.find((c) => c.type === "injected") ?? connectors[0];
  const isPending = status === "connecting" || status === "reconnecting" || connectStatus === "pending";

  return (
    <div className="flex flex-col items-end gap-1">
      <Button
        onClick={() => injectedConnector && connect({ connector: injectedConnector })}
        disabled={!injectedConnector || isPending}
      >
        {isPending ? "Connecting…" : "Connect Wallet"}
      </Button>
      {error && <span className="text-destructive text-xs">{error.message}</span>}
    </div>
  );
}
