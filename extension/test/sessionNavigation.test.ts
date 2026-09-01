import assert from "node:assert/strict";
import test from "node:test";
import {
  awakeSessionDisplay,
  awakeSessionMenuLabel,
  codexSessionRoute
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
