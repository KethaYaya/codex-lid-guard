import * as path from "node:path";
import type { GuardActiveItem } from "./helper";

const codexSessionIdPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export type AwakeSessionDisplay = {
  label: string;
  description: string;
  detail: string;
};

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
  const cwd = item.cwd?.trim() || undefined;
  return {
    cwd,
    workspace: cwd ? path.win32.basename(cwd) || cwd : "Codex session"
  };
}
