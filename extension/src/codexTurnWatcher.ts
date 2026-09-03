import { watch, type FSWatcher } from "node:fs";
import * as fs from "node:fs/promises";
import * as path from "node:path";

const turnStartPattern = /\bReasoning summary turn-start config resolved\b.*\bconversationId=([0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12})(?:\s|$)/iu;
const WATCH_SAFETY_INTERVAL_MS = 100;
const FOCUSED_SESSION_TAIL_BYTES = 256 * 1024;

export type CodexTurnStartWatcher = {
  dispose(): void;
};

export function codexLogPathForExtensionLog(extensionLogDirectory: string): string {
  return path.resolve(extensionLogDirectory, "..", "openai.chatgpt", "Codex.log");
}

export function parseCodexTurnStart(line: string): string | undefined {
  return turnStartPattern.exec(line)?.[1]?.toLowerCase();
}

export function parseFocusedCodexSession(contents: string): string | undefined {
  const lines = contents.split(/\r?\n/u);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index];
    if (!line.includes("thread_stream_view_activity_changed")) {
      continue;
    }
    const active = /\bactive=(true|false)(?:\s|$)/iu.exec(line)?.[1]?.toLowerCase();
    if (active === "false") {
      return undefined;
    }
    if (active === "true") {
      return /\bconversationId=([0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12})(?:\s|$)/iu
        .exec(line)?.[1]?.toLowerCase();
    }
  }
  return undefined;
}

export async function readFocusedCodexSession(logPath: string): Promise<string | undefined> {
  let handle: fs.FileHandle | undefined;
  try {
    handle = await fs.open(logPath, "r");
    const size = (await handle.stat()).size;
    const bytesToRead = Math.min(size, FOCUSED_SESSION_TAIL_BYTES);
    const buffer = Buffer.allocUnsafe(bytesToRead);
    const { bytesRead } = await handle.read(buffer, 0, bytesToRead, size - bytesToRead);
    return parseFocusedCodexSession(buffer.subarray(0, bytesRead).toString("utf8"));
  } catch {
    return undefined;
  } finally {
    await handle?.close().catch(() => undefined);
  }
}

export async function watchCodexTurnStarts(
  logPath: string,
  onTurnStart: (sessionId: string) => void
): Promise<CodexTurnStartWatcher | undefined> {
  const directory = path.dirname(logPath);
  try {
    await fs.access(directory);
  } catch {
    return undefined;
  }

  let offset = (await existingFileSize(logPath)) ?? 0;
  let remainder = "";
  let remainderWasEmitted = false;
  let reading = false;
  let readAgain = false;
  let checkingSize = false;
  let closed = false;
  let watchers: FSWatcher[] = [];
  let watcherRestartTimer: NodeJS.Timeout | undefined;

  const emitTurnStart = (line: string): boolean => {
    const sessionId = parseCodexTurnStart(line);
    if (!sessionId) {
      return false;
    }
    onTurnStart(sessionId);
    return true;
  };

  const readAppended = async (): Promise<void> => {
    if (closed) {
      return;
    }
    if (reading) {
      readAgain = true;
      return;
    }
    reading = true;
    try {
      do {
        readAgain = false;
        let handle: fs.FileHandle | undefined;
        try {
          handle = await fs.open(logPath, "r");
          const size = (await handle.stat()).size;
          if (size < offset) {
            offset = 0;
            remainder = "";
            remainderWasEmitted = false;
          }
          if (size === offset) {
            continue;
          }
          const bytesToRead = size - offset;
          const buffer = Buffer.allocUnsafe(bytesToRead);
          const { bytesRead } = await handle.read(buffer, 0, bytesToRead, offset);
          offset += bytesRead;
          const lines = `${remainder}${buffer.subarray(0, bytesRead).toString("utf8")}`.split(/\r?\n/u);
          const nextRemainder = lines.pop() ?? "";
          for (let index = 0; index < lines.length; index += 1) {
            if (index === 0 && remainderWasEmitted) {
              continue;
            }
            emitTurnStart(lines[index]);
          }
          const nextRemainderWasEmitted = lines.length === 0 && remainderWasEmitted
            ? true
            : emitTurnStart(nextRemainder);
          remainder = nextRemainder;
          remainderWasEmitted = nextRemainderWasEmitted;
        } catch {
          // The official UserPromptSubmit hook remains the authoritative fallback.
        } finally {
          await handle?.close().catch(() => undefined);
        }
      } while (readAgain && !closed);
    } finally {
      reading = false;
      // An event can land after the loop checks readAgain but before reading is
      // cleared. Re-enter here so that append is not stranded until a later log.
      if (readAgain && !closed) {
        void readAppended();
      }
    }
  };

  const scheduleWatcherRestart = (): void => {
    if (closed || watcherRestartTimer) {
      return;
    }
    for (const watcher of watchers) {
      watcher.close();
    }
    watchers = [];
    watcherRestartTimer = setTimeout(() => {
      watcherRestartTimer = undefined;
      startWatcher();
    }, 25);
    watcherRestartTimer.unref();
  };

  const startWatcher = (): void => {
    if (closed) {
      return;
    }
    for (const watcher of watchers) {
      watcher.close();
    }
    watchers = [];
    try {
      watchers.push(watch(logPath, { persistent: false }, (event) => {
        void readAppended();
        if (event === "rename") {
          scheduleWatcherRestart();
        }
      }));
    } catch {}
    try {
      watchers.push(watch(directory, { persistent: false }, (event, filename) => {
        if (!filename || filename.toString().toLowerCase() === "codex.log") {
          void readAppended();
          if (event === "rename") {
            scheduleWatcherRestart();
          }
        }
      }));
    } catch {}
    if (watchers.length === 0) {
      scheduleWatcherRestart();
      return;
    }
    for (const watcher of watchers) {
      watcher.on("error", scheduleWatcherRestart);
    }
  };

  const checkForAppend = async (): Promise<void> => {
    if (closed || checkingSize) {
      return;
    }
    checkingSize = true;
    try {
      const size = await existingFileSize(logPath);
      if (size !== undefined && size !== offset) {
        await readAppended();
      }
    } finally {
      checkingSize = false;
    }
  };

  startWatcher();
  // fs.watch can coalesce or omit notifications on Windows. This inexpensive
  // metadata/read check bounds the delay without scanning Codex transcripts.
  const safetyTimer = setInterval(() => void checkForAppend(), WATCH_SAFETY_INTERVAL_MS);
  safetyTimer.unref();
  void readAppended();

  return {
    dispose(): void {
      closed = true;
      clearInterval(safetyTimer);
      if (watcherRestartTimer) {
        clearTimeout(watcherRestartTimer);
      }
      for (const watcher of watchers) {
        watcher.close();
      }
      watchers = [];
    }
  };
}

async function existingFileSize(filePath: string): Promise<number | undefined> {
  try {
    return (await fs.stat(filePath)).size;
  } catch {
    return undefined;
  }
}
