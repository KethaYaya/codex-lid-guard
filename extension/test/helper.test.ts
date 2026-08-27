import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { readHelperStatus } from "../src/helper";

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
