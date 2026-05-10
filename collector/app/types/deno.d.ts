// Minimal ambient declarations for the Deno globals used by `*.server.ts` /
// resource route modules. The full Deno types (loaded by `deno check` for
// `server.ts`) are richer; this stub exists purely so `tsc --noEmit` can
// type-check files that reference `Deno.*` from within the React Router app.

declare namespace Deno {
  interface Env {
    get(key: string): string | undefined;
  }
  const env: Env;

  interface KvEntry<T> {
    key: readonly unknown[];
    value: T;
    versionstamp: string;
  }
  interface KvEntryMaybe<T> {
    key: readonly unknown[];
    value: T | null;
    versionstamp: string | null;
  }
  interface KvListIterator<T> extends AsyncIterableIterator<KvEntry<T>> {}
  interface AtomicCheck {
    key: readonly unknown[];
    versionstamp: string | null;
  }
  interface AtomicOperation {
    check(...checks: AtomicCheck[]): AtomicOperation;
    set(key: readonly unknown[], value: unknown): AtomicOperation;
    delete(key: readonly unknown[]): AtomicOperation;
    commit(): Promise<{ ok: boolean; versionstamp?: string }>;
  }
  interface Kv {
    get<T = unknown>(key: readonly unknown[]): Promise<KvEntryMaybe<T>>;
    getMany<T extends readonly unknown[]>(
      keys: ReadonlyArray<readonly unknown[]>,
    ): Promise<{ [K in keyof T]: KvEntryMaybe<T[K]> }>;
    set(key: readonly unknown[], value: unknown): Promise<{ ok: boolean; versionstamp: string }>;
    delete(key: readonly unknown[]): Promise<void>;
    list<T = unknown>(
      selector: { prefix: readonly unknown[] },
      options?: { limit?: number; reverse?: boolean; cursor?: string },
    ): KvListIterator<T>;
    atomic(): AtomicOperation;
    close(): void;
  }
  function openKv(path?: string): Promise<Kv>;

  function serve(
    options: { port?: number; hostname?: string; signal?: AbortSignal },
    handler: (request: Request) => Response | Promise<Response>,
  ): { finished: Promise<void>; shutdown(): Promise<void> };
  function serve(handler: (request: Request) => Response | Promise<Response>): {
    finished: Promise<void>;
    shutdown(): Promise<void>;
  };

  function cron(name: string, schedule: string, handler: () => void | Promise<void>): void;

  interface FileInfo {
    isFile: boolean;
    isDirectory: boolean;
    size: number;
  }
  function stat(path: string | URL): Promise<FileInfo>;

  interface FsFile {
    readable: ReadableStream<Uint8Array>;
    close(): void;
  }
  function open(path: string | URL, options?: { read?: boolean }): Promise<FsFile>;
}
