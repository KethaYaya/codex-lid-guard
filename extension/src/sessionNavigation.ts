import * as path from "node:path";
import type { GuardActiveItem, GuardRecentItem } from "./helper";

const codexSessionIdPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

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
