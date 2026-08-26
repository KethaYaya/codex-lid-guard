import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export type GuardStatus = {
  ok: boolean;
  message: string;
  activeTurns: number;
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

export async function writeHelperSettings(
  settingsPath: string,
  alertSounds: boolean,
  sleepWhenLidClosed: boolean,
  sleepDelaySeconds: number
): Promise<void> {
  await fs.mkdir(path.dirname(settingsPath), { recursive: true });
  const settings = {
    alertSounds,
    sleepWhenLidClosed,
    sleepDelaySeconds: Math.max(0, Math.min(300, sleepDelaySeconds))
  };
  await fs.writeFile(settingsPath, `${JSON.stringify(settings, null, 2)}\n`, "utf8");
}
