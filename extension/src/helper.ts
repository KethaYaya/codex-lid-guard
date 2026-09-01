import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export type GuardActiveItem = {
  sessionId: string;
  turnId: string;
  cwd?: string | null;
};

export type GuardStatus = {
  ok: boolean;
  message: string;
  activeTurns: number;
  activeItems?: GuardActiveItem[];
  isGuarding: boolean;
  lidState: "unknown" | "open" | "closed";
  sleepPending: boolean;
};

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
  items: string[]
): Promise<number | undefined> {
  const { stdout } = await execFileAsync(helperPath, ["menu", title, ...items], {
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
