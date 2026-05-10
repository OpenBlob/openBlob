import { createConfig, http } from "wagmi";
import { mainnet, sepolia } from "wagmi/chains";
import { injected } from "wagmi/connectors";

export const wagmiConfig = createConfig({
  chains: [mainnet, sepolia],
  connectors: [injected()],
  ssr: true,
  transports: {
    [mainnet.id]: http(),
    [sepolia.id]: http(),
  },
});

declare module "wagmi" {
  interface Register {
    config: typeof wagmiConfig;
  }
}
