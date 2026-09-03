import assert from "node:assert/strict";
import test from "node:test";
import {
  awakeSessionDisplay,
  awakeSessionMenuLabel,
  codexSessionRoute,
  focusedSessionMenuIndex,
  sessionMenuEntries
} from "../src/sessionNavigation";

test("builds an awake-session item from its Windows workspace", () => {
  const display = awakeSessionDisplay({
    sessionId: "12345678-1234-1234-1234-123456789abc",
    turnId: "turn-1",
    cwd: "C:\\Projects\\CodexLidGuard"
  });
  assert.equal(display.label, "$(comment-discussion) CodexLidGuard");
  assert.equal(display.description, "Session 12345678");
  assert.equal(display.detail, "C:\\Projects\\CodexLidGuard");
  assert.equal(
    awakeSessionMenuLabel({
      sessionId: "12345678-1234-1234-1234-123456789abc",
      turnId: "turn-1",
      cwd: "C:\\Projects\\CodexLidGuard"
    }, "Fix stale Codex session status"),
    "CodexLidGuard — Fix stale Codex session status"
  );
});

test("falls back to the short session ID when a title is unavailable", () => {
  assert.equal(
    awakeSessionMenuLabel({
      sessionId: "12345678-1234-1234-1234-123456789abc",
      turnId: "turn-1",
      cwd: "C:\\Projects\\CodexLidGuard"
    }),
    "CodexLidGuard — Session 12345678"
  );
});

test("builds the installed Codex extension's local-session route", () => {
  assert.equal(
    codexSessionRoute("12345678-1234-1234-1234-123456789abc"),
    "/local/12345678-1234-1234-1234-123456789abc"
  );
  assert.equal(codexSessionRoute("unknown-session"), undefined);
});

test("keeps awake sessions and fills the menu with five recent sessions", () => {
  const active = {
    sessionId: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    turnId: "turn-active",
    cwd: "C:\\Projects\\Active"
  };
  const entries = sessionMenuEntries([active], [
    { sessionId: active.sessionId, cwd: active.cwd, title: "Active task" },
    { sessionId: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", cwd: "C:\\Projects\\Two", title: "Two" },
    { sessionId: "cccccccc-cccc-cccc-cccc-cccccccccccc", cwd: "C:\\Projects\\Three", title: "Three" },
    { sessionId: "dddddddd-dddd-dddd-dddd-dddddddddddd", cwd: "C:\\Projects\\Four", title: "Four" },
    { sessionId: "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee", cwd: "C:\\Projects\\Five", title: "Five" }
  ]);

  assert.equal(entries.length, 5);
  assert.deepEqual(entries.map((entry) => entry.activeItem.sessionId), [
    active.sessionId,
    "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    "cccccccc-cccc-cccc-cccc-cccccccccccc",
    "dddddddd-dddd-dddd-dddd-dddddddddddd",
    "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee"
  ]);
  assert.equal(entries[0].awake, true);
  assert.equal(entries[0].title, "Active task");
  assert.equal(entries[1].awake, false);
  assert.equal(entries[1].activeItem.turnId, "");
});

test("normalizes extended Windows paths in recent-session labels", () => {
  assert.equal(
    awakeSessionMenuLabel({
      sessionId: "12345678-1234-1234-1234-123456789abc",
      turnId: "",
      cwd: "\\\\?\\C:\\Projects\\CodexLidGuard"
    }, "Recent task"),
    "CodexLidGuard — Recent task"
  );
});

test("selects the focused session's menu row case-insensitively", () => {
  const entries = sessionMenuEntries([], [
    { sessionId: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", title: "First" },
    { sessionId: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", title: "Second" }
  ], 2);

  assert.equal(
    focusedSessionMenuIndex(entries, "BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB"),
    1
  );
  assert.equal(
    focusedSessionMenuIndex(entries, "cccccccc-cccc-cccc-cccc-cccccccccccc"),
    undefined
  );
});
