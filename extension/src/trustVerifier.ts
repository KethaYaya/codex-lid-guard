import * as fs from "node:fs/promises";
import * as path from "node:path";

export const guardTrustEvents = ["permission_request", "pre_tool_use", "session_end", "user_prompt_submit", "stop"] as const;
export type GuardTrustEvent = typeof guardTrustEvents[number];
export type GuardTrustHashes = Partial<Record<GuardTrustEvent, string>>;

export function parseGuardTrustHashes(configText: string, hooksPath: string): GuardTrustHashes {
  const hashes: GuardTrustHashes = {};
  const expectedHooksPath = canonicalizeWindowsPath(hooksPath).toLowerCase();
  let currentEvent: GuardTrustEvent | undefined;

  for (const line of configText.split(/\r?\n/)) {
    const header = line.match(/^\[hooks\.state\.'(.+)'\]\s*$/);
    if (header) {
      currentEvent = undefined;
      const stateKey = header[1];
      const keyMatch = stateKey.match(/^(.*):(permission_request|pre_tool_use|session_end|user_prompt_submit|stop):\d+:\d+$/i);
      if (keyMatch && canonicalizeWindowsPath(keyMatch[1]).toLowerCase() === expectedHooksPath) {
        currentEvent = keyMatch[2].toLowerCase() as GuardTrustEvent;
      }
      continue;
    }

    if (!line.startsWith("[") && currentEvent) {
      const trustedHash = line.match(/^trusted_hash\s*=\s*["']([^"']+)["']\s*$/);
      if (trustedHash) {
        hashes[currentEvent] = trustedHash[1];
      }
    } else if (line.startsWith("[")) {
      currentEvent = undefined;
    }
  }

  return hashes;
}

export function allGuardTrustHashesChanged(
  before: GuardTrustHashes,
  after: GuardTrustHashes
): boolean {
  return guardTrustEvents.every((event) => Boolean(after[event]) && after[event] !== before[event]);
}

export function setupStateMatchesRevision(value: string | undefined, revision: string): boolean {
  return value === revision || value?.endsWith(`:${revision}`) === true;
}

export async function readGuardTrustHashes(configPath: string, hooksPath: string): Promise<GuardTrustHashes> {
  try {
    return parseGuardTrustHashes(await fs.readFile(configPath, "utf8"), hooksPath);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return {};
    }
    throw error;
  }
}

function canonicalizeWindowsPath(value: string): string {
  const normalized = path.win32.normalize(value);
  return /^[a-z]:/i.test(normalized)
    ? `${normalized[0].toUpperCase()}${normalized.slice(1)}`
    : normalized;
}
