import * as fs from "node:fs/promises";

type SessionIndexEntry = {
  id?: unknown;
  thread_name?: unknown;
};

export async function readCodexSessionTitles(
  indexPath: string,
  sessionIds: Iterable<string>
): Promise<ReadonlyMap<string, string>> {
  const contents = await fs.readFile(indexPath, "utf8");
  return parseCodexSessionTitles(contents, sessionIds);
}

export function parseCodexSessionTitles(
  contents: string,
  sessionIds: Iterable<string>
): ReadonlyMap<string, string> {
  const requested = new Set(
    Array.from(sessionIds, normalizeSessionId).filter((id) => id.length > 0)
  );
  const titles = new Map<string, string>();

  if (requested.size === 0) {
    return titles;
  }

  for (const line of contents.split(/\r?\n/u)) {
    if (!line.trim()) {
      continue;
    }

    let parsed: unknown;
    try {
      parsed = JSON.parse(line) as unknown;
    } catch {
      continue;
    }

    if (!parsed || typeof parsed !== "object") {
      continue;
    }
    const entry = parsed as SessionIndexEntry;
    if (typeof entry.id !== "string" || typeof entry.thread_name !== "string") {
      continue;
    }

    const id = normalizeSessionId(entry.id);
    const title = entry.thread_name.replace(/\s+/gu, " ").trim();
    if (requested.has(id) && title) {
      // Later index entries represent more recent session-title updates.
      titles.set(id, title);
    }
  }

  return titles;
}

export function codexSessionTitle(
  titles: ReadonlyMap<string, string>,
  sessionId: string
): string | undefined {
  return titles.get(normalizeSessionId(sessionId));
}

function normalizeSessionId(sessionId: string): string {
  return sessionId.trim().toLowerCase();
}
