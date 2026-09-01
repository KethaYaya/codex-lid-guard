import assert from "node:assert/strict";
import test from "node:test";
import {
  canonicalizeWindowsExecutablePath,
  guardHooksForPreference,
  hasGuardHooks,
  hookMarker,
  isOurGroup,
  quotePowerShellLiteral,
  withGuardHooks,
  withoutGuardHooks
} from "../src/hookInstaller";

test("installs all lifecycle and alert hooks while preserving unrelated hooks", () => {
  const original = {
    description: "mine",
    custom: { keep: true },
    hooks: {
      Stop: [{ hooks: [{ type: "command", command: "existing-tool", timeout: 5 }] }]
    }
  };

  const installed = withGuardHooks(original, "C:\\Program Files\\Guard\\CodexLidGuard.exe");
  assert.equal(hasGuardHooks(installed), true);
  assert.deepEqual(installed.custom, { keep: true });
  assert.equal((installed.hooks?.Stop as unknown[]).length, 2);
  assert.match(JSON.stringify(installed), /CodexLidGuard\.exe/);
  assert.match(JSON.stringify(installed), /commandWindows/);
  assert.match(JSON.stringify(installed), /commandWindows[^}]+& 'C:/);
  assert.doesNotMatch(JSON.stringify(installed), /powershell\.exe/);
  assert.match(JSON.stringify(installed), new RegExp(hookMarker));
  assert.match(JSON.stringify(installed.hooks?.PermissionRequest), /sound-request/);
  assert.match(JSON.stringify(installed.hooks?.PreToolUse), /\^request_user_input\$/);
  assert.equal(hasGuardHooks(original), false, "the input object must not be mutated");
});

test("PowerShell paths are quoted as literals", () => {
  assert.equal(quotePowerShellLiteral("C:\\It's Here\\guard.exe"), "'C:\\It''s Here\\guard.exe'");
});

test("Windows drive-letter casing cannot change hook definitions after restart", () => {
  assert.equal(
    canonicalizeWindowsExecutablePath("c:\\Users\\Test\\..\\Test\\Guard.exe"),
    "C:\\Users\\Test\\Guard.exe"
  );

  const lowerCaseDrive = withGuardHooks({}, "c:\\Users\\Test\\Guard.exe");
  const upperCaseDrive = withGuardHooks({}, "C:\\Users\\Test\\Guard.exe");
  assert.deepEqual(lowerCaseDrive, upperCaseDrive);
});

test("installation is idempotent", () => {
  const once = withGuardHooks({}, "C:\\one\\CodexLidGuard.exe");
  const twice = withGuardHooks(once, "C:\\two\\CodexLidGuard.exe");
  for (const event of ["UserPromptSubmit", "PreToolUse", "PermissionRequest", "Stop", "SessionEnd"]) {
    const groups = twice.hooks?.[event] as unknown[];
    assert.equal(groups.filter(isOurGroup).length, 1);
    assert.match(JSON.stringify(groups), /C:\\\\two/);
  }
});

test("uninstall removes only guard-owned groups", () => {
  const existing = { hooks: { Stop: [{ hooks: [{ type: "command", command: "keep-me" }] }] } };
  const installed = withGuardHooks(existing, "C:\\Guard\\CodexLidGuard.exe");
  const removed = withoutGuardHooks(installed);
  assert.equal(hasGuardHooks(removed), false);
  assert.match(JSON.stringify(removed), /keep-me/);
});

test("the default no-hook preference removes previous Lid Guard hooks", () => {
  const existing = { hooks: { Stop: [{ hooks: [{ type: "command", command: "keep-me" }] }] } };
  const installed = withGuardHooks(existing, "C:\\Guard\\CodexLidGuard.exe");
  const defaultPolicy = guardHooksForPreference(
    installed,
    "C:\\Guard\\CodexLidGuard.exe",
    false
  );

  assert.equal(hasGuardHooks(defaultPolicy), false);
  assert.match(JSON.stringify(defaultPolicy), /keep-me/);
});
