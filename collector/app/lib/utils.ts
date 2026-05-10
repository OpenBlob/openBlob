import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatAddress(address: string | undefined | null): string {
  if (!address || address.length < 10) return address ?? "";
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

export async function tryCatch<T>(p: Promise<T>): Promise<[T | null, Error | null]> {
  try {
    const value = await p;
    return [value, null];
  } catch (err) {
    return [null, err as Error];
  }
}
