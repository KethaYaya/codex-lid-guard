import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
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

export type GuardStatus = {
  ok: boolean;
  message: string;
  pipeName?: string | null;
  activeTurns: number;
  activeItems?: GuardActiveItem[];
  isGuarding: boolean;
  lidState: "unknown" | "open" | "closed";
  sleepPending: boolean;
};

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
  action: "status" | "restore" | "sound-done" | "sound-request"
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

export async function showHelperMenu(
  helperPath: string,
  title: string,
  items: string[],
  theme: GuardMenuTheme
): Promise<number | undefined> {
  const { stdout } = await execFileAsync(helperPath, ["menu", `--theme=${theme}`, title, ...items], {
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
  sleepDelaySeconds: number
): Promise<void> {
  await fs.mkdir(path.dirname(settingsPath), { recursive: true });
  const settings = {
    alertSounds,
    alertSoundsOnlyWhenUnfocused,
    sleepWhenLidClosed,
    sleepDelaySeconds: Math.max(0, Math.min(300, sleepDelaySeconds))
  };
  await fs.writeFile(settingsPath, `${JSON.stringify(settings, null, 2)}\n`, "utf8");
}

export async function readHelperStatus(statusPath: string): Promise<GuardStatus> {
  const raw = await fs.readFile(statusPath, "utf8");
  return JSON.parse(raw) as GuardStatus;
}

async function sendGuardianRequest(
  pipeName: string,
  request: Record<string, string | undefined>
): Promise<GuardStatus> {
  const exchange = async (): Promise<GuardStatus> => {
    const handle = await fs.open(pipeName, "r+");
    try {
      await handle.write(`${JSON.stringify(request)}\n`, null, "utf8");
      const chunks: Buffer[] = [];
      let totalBytes = 0;
      while (totalBytes <= 1_048_576) {
        const buffer = Buffer.allocUnsafe(4096);
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
        if (bytesRead === 0) {
          break;
        }
        const chunk = buffer.subarray(0, bytesRead);
        chunks.push(chunk);
        totalBytes += bytesRead;
        const response = Buffer.concat(chunks, totalBytes);
        const newline = response.indexOf(0x0a);
        if (newline >= 0) {
          return JSON.parse(response.subarray(0, newline).toString("utf8")) as GuardStatus;
        }
      }
      throw new Error("The guardian pipe closed without a complete response.");
    } finally {
      await handle.close().catch(() => undefined);
    }
  };

  let timeout: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      exchange(),
      new Promise<never>((_resolve, reject) => {
        timeout = setTimeout(
          () => reject(new Error("The guardian pipe did not respond in time.")),
          DIRECT_ACQUIRE_TIMEOUT_MS
        );
        timeout.unref();
      })
    ]);
  } finally {
    if (timeout) {
      clearTimeout(timeout);
    }
  }
}
