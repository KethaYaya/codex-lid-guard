import assert from "node:assert/strict";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import test from "node:test";
import {
  codexLogPathForExtensionLog,
  parseFocusedCodexSession,
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

test("finds the currently displayed Codex session from view activity", () => {
  const first = "01a05ca7-d20c-7aa2-9fcb-e8bb2a0a78b8";
  const current = "01a05cac-0f8c-7190-9962-fafaefda24ec";
  const log = [
    `thread_stream_view_activity_changed active=true conversationId=${first}`,
    `thread_stream_view_activity_changed active=false conversationId=${first}`,
    `thread_stream_view_activity_changed active=true conversationId=${current}`,
    "unrelated trailing log entry"
  ].join("\n");

  assert.equal(parseFocusedCodexSession(log), current);
});

test("reports no focused session when the latest Codex view is inactive", () => {
  const sessionId = "01a05ca7-d20c-7aa2-9fcb-e8bb2a0a78b8";
  assert.equal(parseFocusedCodexSession([
    `thread_stream_view_activity_changed active=true conversationId=${sessionId}`,
    `thread_stream_view_activity_changed active=false conversationId=${sessionId}`
  ].join("\n")), undefined);
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

test("emits a turn start before the log line receives its trailing newline", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "codex-turn-watcher-partial-"));
  const logPath = path.join(directory, "Codex.log");
  const sessionId = "01a05dc2-c025-7a60-aee6-451599e3a6cb";
  await fs.writeFile(logPath, "", "utf8");

  let watcher: Awaited<ReturnType<typeof watchCodexTurnStarts>> = undefined;
  try {
    const observed: string[] = [];
    watcher = await watchCodexTurnStarts(logPath, (value) => observed.push(value));
    assert.ok(watcher);
    await fs.appendFile(
      logPath,
      `Reasoning summary turn-start config resolved conversationId=${sessionId} reasoningSummaryOverride=null`,
      "utf8"
    );
    await waitFor(() => observed.length === 1);

    await fs.appendFile(logPath, "\nnext log line\n", "utf8");
    await new Promise((resolve) => setTimeout(resolve, 350));
    assert.deepEqual(observed, [sessionId]);
  } finally {
    watcher?.dispose();
    await fs.rm(directory, { recursive: true, force: true });
  }
});

async function waitFor(condition: () => boolean): Promise<void> {
  const deadline = Date.now() + 2000;
  while (!condition()) {
    if (Date.now() >= deadline) {
      throw new Error("turn-start watcher timed out");
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}
