import assert from "node:assert/strict";
import test from "node:test";
import {
  codexSessionTitle,
  parseCodexSessionTitles
} from "../src/sessionIndex";

test("reads the requested Codex session title from the metadata index", () => {
  const titles = parseCodexSessionTitles([
    JSON.stringify({
      id: "01a056eb-e166-7853-9f14-046f31a5835b",
      thread_name: "Fix stale Codex session status",
      updated_at: "2026-08-31T08:25:40Z"
    }),
    JSON.stringify({ id: "another-session", thread_name: "Unrelated title" })
  ].join("\n"), ["01A056EB-E166-7853-9F14-046F31A5835B"]);

  assert.equal(
    codexSessionTitle(titles, "01a056eb-e166-7853-9f14-046f31a5835b"),
    "Fix stale Codex session status"
  );
  assert.equal(titles.size, 1);
});

test("uses the latest title and ignores malformed metadata lines", () => {
  const sessionId = "01a056eb-e166-7853-9f14-046f31a5835b";
  const titles = parseCodexSessionTitles([
    "not json",
    JSON.stringify({ id: sessionId, thread_name: "Old title" }),
    JSON.stringify({ id: sessionId, thread_name: "  Updated\n title  " }),
    JSON.stringify({ id: sessionId, thread_name: "" })
  ].join("\n"), [sessionId]);

  assert.equal(codexSessionTitle(titles, sessionId), "Updated title");
});
