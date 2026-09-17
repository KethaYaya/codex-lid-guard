// Safely finish a local helper update after active Codex turns have drained.
// Run detached; writes a small result file and respects a subsequent tray quit.
import { execFile } from "node:child_process";
import { readFile, writeFile, access } from "node:fs/promises";
import { createConnection } from "node:net";
import path from "node:path";
import { promisify } from "node:util";

const [helper, resultFile] = process.argv.slice(2);
if (!helper || !resultFile || !path.isAbsolute(helper) || !path.isAbsolute(resultFile)) {
  throw new Error("Provide absolute installed-helper and result-file paths.");
}
const exec = promisify(execFile);
const directory = path.join(process.env.LOCALAPPDATA, "CodexLidGuard");
const manifest = JSON.parse(await readFile(path.resolve(path.dirname(helper), "../../package.json"), "utf8"));
const version = manifest.version;
// Long-running turns can outlive a short installation session. Keep the update
// queued through them while still respecting a subsequent explicit tray quit.
const deadline = Date.now() + 24 * 60 * 60_000;
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const atLeast = (value, required) => {
  const actual = String(value).split(".").map(Number);
  const expected = required.split(".").map(Number);
  for (let index = 0; index < 3; index++) {
    if (!Number.isFinite(actual[index])) { return false; }
    if (actual[index] !== expected[index]) { return actual[index] > expected[index]; }
  }
  return true;
};
const record = (state, details = {}) => writeFile(resultFile, JSON.stringify({
  state, targetVersion: version, updaterPid: process.pid, updatedAt: new Date().toISOString(), ...details
}, null, 2));

function statusFromPipe(pipe) {
  if (!/^\\\\\.\\pipe\\CodexLidGuard\.[0-9A-F]{16}$/u.test(pipe)) {
    return Promise.reject(new Error("Invalid guardian pipe"));
  }
  return new Promise((resolve, reject) => {
    const socket = createConnection(pipe);
    let data = "";
    socket.setEncoding("utf8");
    socket.setTimeout(2000, () => socket.destroy(new Error("Status timed out")));
    socket.on("error", reject);
    socket.on("connect", () => socket.write(`${JSON.stringify({ action: "status", clientVersion: version })}\n`));
    socket.on("data", (chunk) => {
      data += chunk;
      if (data.length > 1024 * 1024) { socket.destroy(new Error("Oversized status")); return; }
      if (!data.includes("\n")) { return; }
      try { resolve(JSON.parse(data.slice(0, data.indexOf("\n")))); }
      catch (error) { reject(error); }
      socket.destroy();
    });
    socket.on("end", () => { if (!data.includes("\n")) { reject(new Error("Incomplete status")); } });
  });
}

await record("waiting-for-idle");
let previous;
while (Date.now() < deadline) {
  if (await access(path.join(directory, "helper-paused")).then(() => true, () => false)) {
    await record("cancelled-by-tray-quit");
    process.exit(0);
  }
  try {
    const snapshot = JSON.parse(await readFile(path.join(directory, "status.json"), "utf8"));
    // Query the server itself: a cached file is not proof that a turn has ended.
    let status = await statusFromPipe(snapshot.pipeName);
    if (status.helperPaused) { await record("cancelled-by-tray-quit"); process.exit(0); }
    if (atLeast(status.daemonVersion, version)) {
      await record("ready", { runningVersion: status.daemonVersion, daemonPath: status.daemonPath });
      process.exit(0);
    }
    if (status.activeTurns === 0 && !status.backgroundTasks && !status.sleepPending) {
      // The native client checks the active count again before replacing an
      // older helper, so a newly arriving turn cannot be interrupted here.
      const { stdout } = await exec(helper, ["status"], { windowsHide: true, timeout: 10_000 });
      status = JSON.parse(stdout);
      if (status.ok && atLeast(status.daemonVersion, version)) {
        await record("ready", { runningVersion: status.daemonVersion, daemonPath: status.daemonPath });
        process.exit(0);
      }
    }
    const state = `${status.daemonVersion}:${status.activeTurns}:${status.backgroundTasks ?? 0}:${Boolean(status.sleepPending)}`;
    if (state !== previous) {
      await record("waiting-for-idle", { runningVersion: status.daemonVersion, activeTurns: status.activeTurns,
        backgroundTasks: status.backgroundTasks ?? 0, sleepPending: Boolean(status.sleepPending) });
      previous = state;
    }
  } catch (error) {
    previous = undefined; // Clear a transient connection error on the next healthy status.
    await record("waiting-for-helper", { error: error.message });
  }
  await wait(1000);
}
await record("timed-out");
process.exitCode = 1;
