// OS keyring access for OAuth credentials. Values cross only stdin/stdout, never argv, files or logs.
import type { Credential } from "./credentials";
import { SafeError } from "./diagnostics";

/** Where one credential record per issued client id is kept. */
export interface CredentialStore {
  save(clientId: string, cred: Credential): Promise<void>;
  load(clientId: string): Promise<Credential | null>;
  remove(clientId: string): Promise<void>;
}

const SERVICE = "nzube-subscription-proof";

/**
 * Linux Secret Service (GNOME Keyring, KWallet) through `secret-tool` from libsecret.
 * Other platforms need their own implementation (macOS Keychain, Windows Credential Manager).
 */
export const secretServiceStore: CredentialStore = {
  async save(clientId, cred) {
    const p = Bun.spawn(["secret-tool", "store", "--label=Nzube subscription proof", "service", SERVICE, "client_id", clientId], {
      stdin: new Blob([JSON.stringify(cred)]),
      stderr: "pipe",
    });
    if ((await p.exited) !== 0) throw new SafeError("storage_error", "keyring store failed");
  },
  async load(clientId) {
    const p = Bun.spawn(["secret-tool", "lookup", "service", SERVICE, "client_id", clientId], { stdout: "pipe", stderr: "pipe" });
    const out = await new Response(p.stdout).text();
    if ((await p.exited) !== 0 || !out) return null;
    return JSON.parse(out) as Credential;
  },
  async remove(clientId) {
    await Bun.spawn(["secret-tool", "clear", "service", SERVICE, "client_id", clientId]).exited;
  },
};

/** Test-only store that lives in process memory and never touches disk or the OS keyring. */
export function memoryStore(): CredentialStore {
  const records = new Map<string, Credential>();
  return {
    async save(clientId, cred) {
      records.set(clientId, cred);
    },
    async load(clientId) {
      return records.get(clientId) ?? null;
    },
    async remove(clientId) {
      records.delete(clientId);
    },
  };
}

/** The OS keyring, unless NZUBE_PROOF_KEYRING=memory selects the test-only store. */
export function selectedStore(): CredentialStore {
  return process.env.NZUBE_PROOF_KEYRING === "memory" ? memoryStore() : secretServiceStore;
}
