import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import * as fs from "node:fs/promises";
import { createConnection } from "node:net";
import * as path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const GUARDIAN_PIPE_PATTERN = /^\\\\\.\\pipe\\CodexLidGuard\.[0-9A-F]{16}$/u;
const DIRECT_ACQUIRE_TIMEOUT_MS = 250;

export type GuardActiveItem = {
  sessionId: string;
  turnId: string;
  cwd?: string | null;
};

export type GuardRecentItem = {
  sessionId: string;
  cwd?: string | null;
  title?: string | null;
};

export type GuardStatus = {
  ok: boolean;
  message: string;
  daemonPath?: string | null;
  daemonVersion?: string | null;
  pipeName?: string | null;
  activeTurns: number;
  activeItems?: GuardActiveItem[];
  recentItems?: GuardRecentItem[];
  isGuarding: boolean;
  lidState: "unknown" | "open" | "closed";
  sleepPending: boolean;
};

export function daemonHandoffRequired(
  status: GuardStatus,
  currentVersion: string
): boolean {
  if (status.activeTurns !== 0) {
    return false;
  }
  const candidate = parseReleaseVersion(status.daemonVersion);
  const current = parseReleaseVersion(currentVersion);
  if (!candidate || !current) {
    return status.daemonVersion !== currentVersion;
  }
  for (let index = 0; index < current.length; index += 1) {
    if (candidate[index] !== current[index]) {
      return candidate[index] < current[index];
    }
  }
  return false;
}

function parseReleaseVersion(value: string | null | undefined): [number, number, number] | undefined {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:[-+]|$)/u.exec(value ?? "");
  if (!match) {
    return undefined;
  }
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

export function isGuardianPipeName(value: string | null | undefined): value is string {
  return typeof value === "string" && GUARDIAN_PIPE_PATTERN.test(value);
}

export async function warmGuardianPipe(
  pipeName: string,
  clientVersion: string
): Promise<void> {
  if (!isGuardianPipeName(pipeName)) {
    throw new Error("The guardian returned an invalid local pipe name.");
  }
  const status = await sendGuardianRequest(pipeName, {
    action: "status",
    clientVersion
  });
  if (!status.ok) {
    throw new Error(status.message || "The guardian pipe warm-up failed.");
  }
}

export async function preAcquireGuardian(
  pipeName: string,
  clientVersion: string,
  sessionId: string,
  pendingTurnId: string,
  cwd?: string
): Promise<GuardStatus> {
  if (!isGuardianPipeName(pipeName)) {
    throw new Error("The guardian returned an invalid local pipe name.");
  }
  return sendGuardianRequest(pipeName, {
    action: "pre-acquire",
    clientVersion,
    sessionId,
    turnId: pendingTurnId,
    cwd
  });
}

export type GuardMenuTheme =
  | "dark"
  | "light"
  | "high-contrast"
  | "high-contrast-light";

export async function runHelper(
  helperPath: string,
  action: "status" | "status-with-recent" | "restore" | "sound-done" | "sound-request"
): Promise<GuardStatus> {
  const args = action.startsWith("sound-") ? ["sound", action.slice("sound-".length)] : [action];
  const { stdout } = await execFileAsync(helperPath, args, {
    windowsHide: true,
    timeout: 7000,
    encoding: "utf8"
  });
  return JSON.parse(stdout) as GuardStatus;
}

export async function preAcquireHelper(
  helperPath: string,
  sessionId: string,
  pendingTurnId: string,
  cwd?: string
): Promise<GuardStatus> {
  const args = ["pre-acquire", sessionId, pendingTurnId];
  if (cwd) {
    args.push(cwd);
  }
  const { stdout } = await execFileAsync(helperPath, args, {
    windowsHide: true,
    timeout: 7000,
    encoding: "utf8"
  });
  return JSON.parse(stdout) as GuardStatus;
}

export async function focusHelperSession(
  helperPath: string,
  sessionId: string,
  turnId: string
): Promise<GuardStatus> {
  const { stdout } = await execFileAsync(helperPath, ["focus", sessionId, turnId], {
    windowsHide: true,
    timeout: 7000,
    encoding: "utf8"
  });
  return JSON.parse(stdout) as GuardStatus;
}

export async function associateHelperSession(
  helperPath: string,
  sessionId: string
): Promise<GuardStatus> {
  const { stdout } = await execFileAsync(helperPath, ["associate-window", sessionId], {
    windowsHide: true,
    timeout: 7000,
    encoding: "utf8"
  });
  return JSON.parse(stdout) as GuardStatus;
}

export async function showHelperMenu(
  helperPath: string,
  title: string,
  items: string[],
  theme: GuardMenuTheme,
  initialIndex?: number,
  activeIndices: readonly number[] = [],
  unviewedCompletedIndices: readonly number[] = []
): Promise<number | undefined> {
  const args = ["menu", `--theme=${theme}`];
  if (initialIndex !== undefined && Number.isInteger(initialIndex)
      && initialIndex >= 0 && initialIndex < items.length) {
    args.push(`--selected=${initialIndex}`);
  }
  const validActiveIndices = activeIndices.filter(
    (index) => Number.isInteger(index) && index >= 0 && index < items.length
  );
  if (validActiveIndices.length > 0) {
    args.push(`--active=${validActiveIndices.join(",")}`);
  }
  const validUnviewedCompletedIndices = unviewedCompletedIndices.filter(
    (index) => Number.isInteger(index) && index >= 0 && index < items.length
  );
  if (validUnviewedCompletedIndices.length > 0) {
    args.push(`--unviewed=${validUnviewedCompletedIndices.join(",")}`);
  }
  args.push(title, ...items);
  const { stdout } = await execFileAsync(helperPath, args, {
    windowsHide: true,
    encoding: "utf8"
  });
  const result = JSON.parse(stdout) as { selectedIndex?: number | null };
  if (result.selectedIndex === null || result.selectedIndex === undefined) {
    return undefined;
  }
  if (!Number.isInteger(result.selectedIndex) || result.selectedIndex < 0 || result.selectedIndex >= items.length) {
    throw new Error("The native awake-session menu returned an invalid selection.");
  }
  return result.selectedIndex;
}

export async function writeHelperSettings(
  settingsPath: string,
  alertSounds: boolean,
  alertSoundsOnlyWhenUnfocused: boolean,
  sleepWhenLidClosed: boolean,
  sleepDelaySeconds: number,
  messageOverlay = false,
  overlayOpacity = 82,
  overlayDurationSeconds = 90,
  overlayPosition = "bottom-right"
): Promise<void> {
  await fs.mkdir(path.dirname(settingsPath), { recursive: true });
  const settings = {
    messageOverlay,
    overlayOpacity: Math.round(Math.max(30, Math.min(100, overlayOpacity))),
    overlayDurationSeconds: Math.round(Math.max(10, Math.min(600, overlayDurationSeconds))),
    overlayPosition,
    alertSounds,
    alertSoundsOnlyWhenUnfocused,
    sleepWhenLidClosed,
    sleepDelaySeconds: Math.max(0, Math.min(300, sleepDelaySeconds))
  };
  const temporaryPath = `${settingsPath}.${process.pid}.${randomUUID()}.tmp`;
  try {
    await fs.writeFile(temporaryPath, `${JSON.stringify(settings, null, 2)}\n`, "utf8");
    await fs.rename(temporaryPath, settingsPath);
  } finally {
    await fs.unlink(temporaryPath).catch(() => undefined);
  }
}

export async function previewHelperOverlay(helperPath: string): Promise<void> {
  await execFileAsync(helperPath, ["overlay-preview"], { windowsHide: true, timeout: 45000 });
}

export async function readHelperStatus(statusPath: string): Promise<GuardStatus> {
  const raw = await fs.readFile(statusPath, "utf8");
  return JSON.parse(raw) as GuardStatus;
}

async function sendGuardianRequest(
  pipeName: string,
  request: Record<string, string | undefined>
): Promise<GuardStatus> {
  return new Promise<GuardStatus>((resolve, reject) => {
    // Destroying a socket cancels a pending Windows pipe read on timeout.
    const socket = createConnection(pipeName);
    const chunks: Buffer[] = [];
    let totalBytes = 0;
    let settled = false;
    const finish = (error?: Error, status?: GuardStatus): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      socket.destroy();
      if (error) reject(error);
      else resolve(status!);
    };
    const timeout = setTimeout(
      () => finish(new Error("The guardian pipe did not respond in time.")),
      DIRECT_ACQUIRE_TIMEOUT_MS
    );
    timeout.unref();
    socket.once("connect", () => {
      // Arm libuv's pipe reader before the native server can reply and disconnect.
      setImmediate(() => {
        if (!settled) socket.write(`${JSON.stringify(request)}\n`);
      });
    });
    socket.once("error", finish);
    socket.once("close", () => finish(new Error("The guardian pipe closed without a complete response.")));
    socket.on("data", (chunk: Buffer) => {
      if (settled) return;
      const newline = chunk.indexOf(0x0a);
      const part = newline >= 0 ? chunk.subarray(0, newline) : chunk;
      totalBytes += part.length;
      if (totalBytes > 1_048_576) {
        finish(new Error("The guardian pipe response exceeded the size limit."));
        return;
      }
      chunks.push(part);
      if (newline >= 0) {
        try {
          finish(undefined, JSON.parse(Buffer.concat(chunks, totalBytes).toString("utf8")) as GuardStatus);
        } catch (error) {
          finish(error instanceof Error ? error : new Error(String(error)));
        }
      }
    });
  });
}
