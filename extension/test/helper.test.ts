import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { readHelperStatus, writeHelperSettings } from "../src/helper";

test("reads an atomic guardian status snapshot", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "codex-lid-guard-test-"));
  const statusPath = path.join(directory, "status.json");
  try {
    await writeFile(statusPath, JSON.stringify({
      ok: true,
      message: "ready",
      activeTurns: 2,
      isGuarding: true,
      lidState: "closed",
      sleepPending: false
    }));
    const status = await readHelperStatus(statusPath);
    assert.equal(status.activeTurns, 2);
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
