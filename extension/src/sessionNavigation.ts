import * as path from "node:path";
import type { GuardActiveItem, GuardRecentItem } from "./helper";

const codexSessionIdPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_UNVIEWED_COMPLETED_SESSIONS = 100;

export type AwakeSessionDisplay = {
  label: string;
  description: string;
  detail: string;
};

export type SessionMenuEntry = {
  activeItem: GuardActiveItem;
  title?: string;
  awake: boolean;
};

export type SessionAttentionState = {
  activeSessionIds: string[];
  unviewedCompletedSessionIds: string[];
};

export function normalizedSessionAttentionState(value: unknown): SessionAttentionState {
  const candidate = value && typeof value === "object"
    ? value as Partial<SessionAttentionState>
    : {};
  return {
    activeSessionIds: normalizedSessionIds(candidate.activeSessionIds),
    unviewedCompletedSessionIds: normalizedSessionIds(candidate.unviewedCompletedSessionIds)
      .slice(-MAX_UNVIEWED_COMPLETED_SESSIONS)
  };
}

export function updatedSessionAttention(
  previous: SessionAttentionState,
  activeSessionIds: readonly string[] | undefined,
  viewedSessionIds: readonly string[] = []
): SessionAttentionState {
  const previousActive = normalizedSessionIds(previous.activeSessionIds);
  const active = activeSessionIds === undefined
    ? previousActive
    : normalizedSessionIds(activeSessionIds);
  const activeSet = new Set(active);
  const viewedSet = new Set(normalizedSessionIds(viewedSessionIds));
  const unviewed = new Set(normalizedSessionIds(previous.unviewedCompletedSessionIds));

  for (const sessionId of activeSet) {
    unviewed.delete(sessionId);
  }
  for (const sessionId of viewedSet) {
    unviewed.delete(sessionId);
  }
  if (activeSessionIds !== undefined) {
    for (const sessionId of previousActive) {
      if (!activeSet.has(sessionId) && !viewedSet.has(sessionId)) {
        unviewed.add(sessionId);
      }
    }
  }

  return {
    activeSessionIds: active,
    unviewedCompletedSessionIds: [...unviewed].slice(-MAX_UNVIEWED_COMPLETED_SESSIONS)
  };
}

export function unviewedCompletedMenuIndices(
  entries: readonly SessionMenuEntry[],
  sessionIds: readonly string[]
): number[] {
  const unviewed = new Set(normalizedSessionIds(sessionIds));
  return entries.flatMap((entry, index) => {
    const sessionId = normalizedSessionId(entry.activeItem.sessionId);
    return !entry.awake && unviewed.has(sessionId) ? [index] : [];
  });
}

export function sessionMenuEntries(
  activeItems: readonly GuardActiveItem[],
  recentItems: readonly GuardRecentItem[],
  limit = 5
): SessionMenuEntry[] {
  const entries: SessionMenuEntry[] = [];
  const included = new Set<string>();

  for (const activeItem of activeItems) {
    const sessionId = normalizedSessionId(activeItem.sessionId);
    if (!sessionId || included.has(sessionId)) {
      continue;
    }
    const recent = recentItems.find(
      (item) => normalizedSessionId(item.sessionId) === sessionId
    );
    entries.push({
      activeItem,
      title: normalizedTitle(recent?.title ?? undefined),
      awake: true
    });
    included.add(sessionId);
  }

  const targetSize = Math.max(entries.length, Math.max(0, Math.trunc(limit)));
  for (const recent of recentItems) {
    if (entries.length >= targetSize) {
      break;
    }
    const sessionId = normalizedSessionId(recent.sessionId);
    if (!sessionId || included.has(sessionId)) {
      continue;
    }
    entries.push({
      activeItem: {
        sessionId: recent.sessionId,
        turnId: "",
        cwd: recent.cwd
      },
      title: normalizedTitle(recent.title ?? undefined),
      awake: false
    });
    included.add(sessionId);
  }
  return entries;
}

export function focusedSessionMenuIndex(
  entries: readonly SessionMenuEntry[],
  sessionId: string | undefined
): number | undefined {
  const normalized = normalizedSessionId(sessionId ?? "");
  if (!normalized) {
    return undefined;
  }
  const index = entries.findIndex(
    (entry) => normalizedSessionId(entry.activeItem.sessionId) === normalized
  );
  return index >= 0 ? index : undefined;
}

export function awakeSessionDisplay(
  item: GuardActiveItem,
  sessionTitle?: string
): AwakeSessionDisplay {
  const { cwd, workspace } = sessionLocation(item);
  return {
    label: `$(comment-discussion) ${workspace}`,
    description: normalizedTitle(sessionTitle) ?? `Session ${shortId(item.sessionId)}`,
    detail: cwd || `Codex session ${item.sessionId}`
  };
}

export function awakeSessionMenuLabel(item: GuardActiveItem, sessionTitle?: string): string {
  const { workspace } = sessionLocation(item);
  const title = normalizedTitle(sessionTitle) ?? `Session ${shortId(item.sessionId)}`;
  return `${workspace} — ${title}`;
}

export function codexSessionRoute(sessionId: string): string | undefined {
  const normalized = sessionId.trim();
  return codexSessionIdPattern.test(normalized)
    ? `/local/${encodeURIComponent(normalized)}`
    : undefined;
}

function shortId(value: string): string {
  const normalized = value.trim();
  return normalized.length > 8 ? normalized.slice(0, 8) : normalized;
}

function normalizedTitle(value: string | undefined): string | undefined {
  const normalized = value?.replace(/\s+/gu, " ").trim();
  return normalized || undefined;
}

function sessionLocation(item: GuardActiveItem): { cwd: string | undefined; workspace: string } {
  const cwd = item.cwd?.trim().replace(/^\\\\\?\\/u, "") || undefined;
  return {
    cwd,
    workspace: cwd ? path.win32.basename(cwd) || cwd : "Codex session"
  };
}

function normalizedSessionId(value: string): string {
  return value.trim().toLowerCase();
}

function normalizedSessionIds(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const normalized = new Set<string>();
  for (const candidate of value) {
    if (typeof candidate !== "string") {
      continue;
    }
    const sessionId = normalizedSessionId(candidate);
    if (sessionId) {
      normalized.add(sessionId);
    }
  }
  return [...normalized];
}
