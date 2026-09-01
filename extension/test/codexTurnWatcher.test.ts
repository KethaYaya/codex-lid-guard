import assert from "node:assert/strict";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import test from "node:test";
import {
  codexLogPathForExtensionLog,
  parseCodexTurnStart,
  watchCodexTurnStarts
} from "../src/codexTurnWatcher";

test("recognizes a Codex turn-start event without reading message content", () => {
  const sessionId = "01a05ca7-d20c-7aa2-9fcb-e8bb2a0a78b8";
  const line = `2026-09-01 16:42:55.156 [info] Reasoning summary turn-start config resolved conversationId=${sessionId} reasoningSummaryOverride=null`;

  assert.equal(parseCodexTurnStart(line), sessionId);
  assert.equal(parseCodexTurnStart(`prompt text conversationId=${sessionId}`), undefined);
});

test("locates the Codex log beside the current extension-host log", () => {
  const result = codexLogPathForExtensionLog(
    "C:\\Users\\Test\\AppData\\Roaming\\Code\\logs\\run\\window1\\exthost\\kethayaya.codex-lid-guard"
  );

  assert.equal(
    result,
    "C:\\Users\\Test\\AppData\\Roaming\\Code\\logs\\run\\window1\\exthost\\openai.chatgpt\\Codex.log"
  );
});

test("watches only newly appended Codex turn-start events", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "codex-turn-watcher-"));
  const logPath = path.join(directory, "Codex.log");
  const previousSession = "01a05ca7-d20c-7aa2-9fcb-e8bb2a0a78b8";
  const nextSession = "01a05cac-0f8c-7190-9962-fafaefda24ec";
  await fs.writeFile(
    logPath,
    `Reasoning summary turn-start config resolved conversationId=${previousSession}\n`,
    "utf8"
  );

  let watcher: Awaited<ReturnType<typeof watchCodexTurnStarts>> = undefined;
  try {
    let observeTurnStart: ((sessionId: string) => void) | undefined;
    const observed = new Promise<string>((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("turn-start watcher timed out")), 2000);
      observeTurnStart = (sessionId) => {
        clearTimeout(timeout);
        resolve(sessionId);
      };
    });
    watcher = await watchCodexTurnStarts(logPath, (sessionId) => observeTurnStart?.(sessionId));
    assert.ok(watcher);
    await fs.appendFile(
      logPath,
      `Reasoning summary turn-start config resolved conversationId=${nextSession}\n`,
      "utf8"
    );
    assert.equal(await observed, nextSession);
  } finally {
    watcher?.dispose();
    await fs.rm(directory, { recursive: true, force: true });
  }
});
