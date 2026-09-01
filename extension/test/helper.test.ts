import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import * as net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  isGuardianPipeName,
  preAcquireGuardian,
  readHelperStatus,
  warmGuardianPipe,
  writeHelperSettings
} from "../src/helper";

test("accepts only the per-user local guardian pipe format", () => {
  assert.equal(isGuardianPipeName("\\\\.\\pipe\\CodexLidGuard.0123456789ABCDEF"), true);
  assert.equal(isGuardianPipeName("\\\\.\\pipe\\unrelated.0123456789ABCDEF"), false);
  assert.equal(isGuardianPipeName("C:\\temp\\pipe"), false);
});

test("warms and pre-acquires directly through the running guardian pipe", {
  skip: process.platform !== "win32"
}, async () => {
  const pipeName = `\\\\.\\pipe\\CodexLidGuard.${randomBytes(8).toString("hex").toUpperCase()}`;
  const requests: Array<Record<string, string>> = [];
  const server = net.createServer((socket) => {
    let input = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      input += chunk;
      const newline = input.indexOf("\n");
      if (newline < 0) {
        return;
      }
      const request = JSON.parse(input.slice(0, newline)) as Record<string, string>;
      requests.push(request);
      socket.end(`${JSON.stringify({
        ok: true,
        message: "awake",
        pipeName,
        activeTurns: 1,
        isGuarding: true,
        lidState: "open",
        sleepPending: false
      })}\n`);
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(pipeName, resolve);
  });
  try {
    await warmGuardianPipe(pipeName, "0.1.22");
    const status = await preAcquireGuardian(
      pipeName,
      "0.1.22",
      "01a056eb-e166-7853-9f14-046f31a5835b",
      "pending-fast",
      "C:\\workspace"
    );

    assert.equal(status.isGuarding, true);
    assert.equal(requests[0]?.action, "status");
    assert.equal(requests[1]?.action, "pre-acquire");
    assert.equal(requests[1]?.clientVersion, "0.1.22");
    assert.equal(requests[1]?.turnId, "pending-fast");
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

test("reads an atomic guardian status snapshot", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "codex-lid-guard-test-"));
  const statusPath = path.join(directory, "status.json");
  try {
    await writeFile(statusPath, JSON.stringify({
      ok: true,
      message: "ready",
      activeTurns: 2,
      activeItems: [{
        sessionId: "12345678-1234-1234-1234-123456789abc",
        turnId: "turn-1",
        cwd: "C:\\workspace"
      }],
      isGuarding: true,
      lidState: "closed",
      sleepPending: false
    }));
    const status = await readHelperStatus(statusPath);
    assert.equal(status.activeTurns, 2);
    assert.equal(status.activeItems?.[0]?.sessionId, "12345678-1234-1234-1234-123456789abc");
    assert.equal(status.isGuarding, true);
    assert.equal(status.lidState, "closed");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("writes the background-only alert setting for the native guardian", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "codex-lid-guard-settings-test-"));
  const settingsPath = path.join(directory, "settings.json");
  try {
    await writeHelperSettings(settingsPath, true, true, true, 10);
    const settings = JSON.parse(await readFile(settingsPath, "utf8"));
    assert.equal(settings.alertSounds, true);
    assert.equal(settings.alertSoundsOnlyWhenUnfocused, true);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
