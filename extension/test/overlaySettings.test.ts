import { test } from "node:test";
import * as assert from "node:assert/strict";
import { overlayShortcutProblem } from "../src/overlaySettings";

const defaults = { enabled: true, prefix: "Copilot", cycleKey: "Tab", openKey: "Enter", closeKey: "Escape" };

test("accepts remapped global prefixes and distinct action keys", () => {
  for (const prefix of ["Copilot", "Ctrl+Alt+Space", "alt+shift+g", "Win+F12", "Ctrl+1"]) {
    assert.equal(overlayShortcutProblem({ ...defaults, prefix, cycleKey: "Down", openKey: "Right", closeKey: "Delete" }), undefined);
  }
});

test("rejects ordinary typing prefixes, duplicate modifiers and ambiguous actions", () => {
  for (const prefix of ["", "D", "Shift+D", "Ctrl+Ctrl+D", "Ctrl+F25", "Ctrl++A", "Ctrl+Alt"]) {
    assert.ok(overlayShortcutProblem({ ...defaults, prefix }));
  }
  assert.ok(overlayShortcutProblem({ ...defaults, openKey: "tab" }));
  assert.ok(overlayShortcutProblem({ ...defaults, closeKey: "D" }));
  assert.equal(overlayShortcutProblem({ ...defaults, enabled: false, prefix: "" }), undefined);
});
