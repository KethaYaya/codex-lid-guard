import { watch, type FSWatcher } from "node:fs";
import * as fs from "node:fs/promises";
import * as path from "node:path";

const turnStartPattern = /\bReasoning summary turn-start config resolved\b.*\bconversationId=([0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12})(?:\s|$)/iu;

export type CodexTurnStartWatcher = {
  dispose(): void;
};

export function codexLogPathForExtensionLog(extensionLogDirectory: string): string {
  return path.resolve(extensionLogDirectory, "..", "openai.chatgpt", "Codex.log");
}

export function parseCodexTurnStart(line: string): string | undefined {
  return turnStartPattern.exec(line)?.[1]?.toLowerCase();
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

  let offset = await fileSize(logPath);
  let remainder = "";
  let reading = false;
  let readAgain = false;
  let closed = false;

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
          }
          if (size === offset) {
            continue;
          }
          const bytesToRead = size - offset;
          const buffer = Buffer.allocUnsafe(bytesToRead);
          const { bytesRead } = await handle.read(buffer, 0, bytesToRead, offset);
          offset += bytesRead;
          const lines = `${remainder}${buffer.subarray(0, bytesRead).toString("utf8")}`.split(/\r?\n/u);
          remainder = lines.pop() ?? "";
          for (const line of lines) {
            const sessionId = parseCodexTurnStart(line);
            if (sessionId) {
              onTurnStart(sessionId);
            }
          }
        } catch {
          // The official UserPromptSubmit hook remains the authoritative fallback.
        } finally {
          await handle?.close().catch(() => undefined);
        }
      } while (readAgain && !closed);
    } finally {
      reading = false;
    }
  };

  let watcher: FSWatcher;
  try {
    watcher = watch(directory, { persistent: false }, (_event, filename) => {
      if (!filename || filename.toString().toLowerCase() === "codex.log") {
        void readAppended();
      }
    });
  } catch {
    return undefined;
  }
  watcher.on("error", () => watcher.close());
  void readAppended();

  return {
    dispose(): void {
      closed = true;
      watcher.close();
    }
  };
}

async function fileSize(filePath: string): Promise<number> {
  try {
    return (await fs.stat(filePath)).size;
  } catch {
    return 0;
  }
}
