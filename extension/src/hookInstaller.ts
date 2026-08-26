import * as fs from "node:fs/promises";
import * as path from "node:path";

export const hookMarker = "codex-lid-guard:vscode-extension";

type HookCommand = {
  type: "command";
  command: string;
  commandWindows: string;
  timeout: number;
  statusMessage?: string;
};

type HookGroup = {
  matcher?: string;
  hooks: HookCommand[];
};

export type HooksDocument = {
  description?: string;
  hooks?: Record<string, unknown>;
  [key: string]: unknown;
};

const eventActions: ReadonlyArray<[string, string, string, number, string?]> = [
  ["UserPromptSubmit", "acquire", "Keeping Windows awake for this Codex turn", 5],
  ["PreToolUse", "sound-request", "Playing the Codex needs-response alert", 3, "^request_user_input$"],
  ["PermissionRequest", "sound-request", "Playing the Codex needs-response alert", 3],
  ["Stop", "release", "Restoring Windows sleep behavior", 5],
  ["SessionEnd", "release-session", "Cleaning up Codex Lid Guard", 3]
];

export function quoteWindowsCommandArgument(value: string): string {
  return `"${value.replaceAll('"', '\\"')}"`;
}

export function quotePowerShellLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

export function canonicalizeWindowsExecutablePath(value: string): string {
  const normalized = path.win32.normalize(value);
  if (/^[a-z]:/i.test(normalized)) {
    return `${normalized[0].toUpperCase()}${normalized.slice(1)}`;
  }
  return normalized;
}

export function isOurGroup(value: unknown): boolean {
  if (!value || typeof value !== "object" || !("hooks" in value)) {
    return false;
  }
  const commands = (value as { hooks?: unknown }).hooks;
  return Array.isArray(commands) && commands.some((candidate) => {
    if (!candidate || typeof candidate !== "object" || !("command" in candidate)) {
      return false;
    }
    return String((candidate as { command?: unknown }).command).includes(hookMarker);
  });
}

export function withGuardHooks(document: HooksDocument, helperPath: string): HooksDocument {
  const next = structuredClone(document);
  const hooks = isRecord(next.hooks) ? next.hooks : {};
  next.hooks = hooks;
  const canonicalHelperPath = canonicalizeWindowsExecutablePath(helperPath);
  const executable = quoteWindowsCommandArgument(canonicalHelperPath);
  const powershellExecutable = quotePowerShellLiteral(canonicalHelperPath);

  for (const [event, action, statusMessage, timeout, matcher] of eventActions) {
    const existing = Array.isArray(hooks[event]) ? hooks[event] as unknown[] : [];
    const retained = existing.filter((group) => !isOurGroup(group));
    const command: HookCommand = {
      type: "command",
      command: `${executable} hook ${action} --source ${hookMarker}`,
      commandWindows: `& ${powershellExecutable} hook ${action} --source '${hookMarker}'`,
      timeout,
      statusMessage
    };
    const group: HookGroup = { hooks: [command] };
    if (matcher) {
      group.matcher = matcher;
    }
    hooks[event] = [...retained, group];
  }

  return next;
}

export function withoutGuardHooks(document: HooksDocument): HooksDocument {
  const next = structuredClone(document);
  if (!isRecord(next.hooks)) {
    return next;
  }

  for (const [event] of eventActions) {
    const groups = next.hooks[event];
    if (Array.isArray(groups)) {
      next.hooks[event] = groups.filter((group) => !isOurGroup(group));
    }
  }
  return next;
}

export function hasGuardHooks(document: HooksDocument): boolean {
  if (!isRecord(document.hooks)) {
    return false;
  }
  return eventActions.every(([event]) => {
    const groups = document.hooks?.[event];
    return Array.isArray(groups) && groups.some(isOurGroup);
  });
}

export async function readHooksDocument(hooksPath: string): Promise<HooksDocument> {
  try {
    const raw = await fs.readFile(hooksPath, "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      throw new Error("the root value must be a JSON object");
    }
    return parsed as HooksDocument;
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ENOENT") {
      return {};
    }
    if (error instanceof SyntaxError) {
      throw new Error(`Cannot update ${hooksPath} because it contains invalid JSON: ${error.message}`);
    }
    throw error;
  }
}

export async function writeHooksDocument(hooksPath: string, document: HooksDocument): Promise<void> {
  await fs.mkdir(path.dirname(hooksPath), { recursive: true });
  try {
    await fs.copyFile(hooksPath, `${hooksPath}.before-codex-lid-guard`);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      throw error;
    }
  }
  const temporary = `${hooksPath}.${process.pid}.tmp`;
  await fs.writeFile(temporary, `${JSON.stringify(document, null, 2)}\n`, "utf8");
  await fs.rename(temporary, hooksPath);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
