import assert from "node:assert/strict";
import test from "node:test";
import {
  allGuardTrustHashesChanged,
  parseGuardTrustHashes
} from "../src/trustVerifier";

const hooksPath = "C:\\Users\\Test\\.codex\\hooks.json";

test("reads only the five Lid Guard trust hashes for the selected hooks file", () => {
  const config = `
[hooks.state.'C:\\Users\\Test\\.codex\\hooks.json:session_start:0:0']
trusted_hash = "ignore"

[hooks.state.'c:\\Users\\Test\\.codex\\hooks.json:session_end:0:0']
trusted_hash = "end"

[hooks.state.'C:\\Users\\Test\\.codex\\hooks.json:permission_request:0:0']
trusted_hash = "request"

[hooks.state.'C:\\Users\\Test\\.codex\\hooks.json:pre_tool_use:0:0']
trusted_hash = "input"

[hooks.state.'C:\\Users\\Test\\.codex\\hooks.json:user_prompt_submit:1:0']
trusted_hash = "prompt"

[hooks.state.'C:\\Users\\Test\\.codex\\hooks.json:stop:2:0']
trusted_hash = "stop"
`;

  assert.deepEqual(parseGuardTrustHashes(config, hooksPath), {
    permission_request: "request",
    pre_tool_use: "input",
    session_end: "end",
    user_prompt_submit: "prompt",
    stop: "stop"
  });
});

test("requires every Lid Guard hash to change before setup is complete", () => {
  const before = { permission_request: "old-request", pre_tool_use: "old-input", session_end: "old-end", user_prompt_submit: "old-prompt", stop: "old-stop" };
  assert.equal(allGuardTrustHashesChanged(before, {
    permission_request: "new-request",
    pre_tool_use: "new-input",
    session_end: "new-end",
    user_prompt_submit: "new-prompt",
    stop: "new-stop"
  }), true);
  assert.equal(allGuardTrustHashesChanged(before, {
    permission_request: "new-request",
    pre_tool_use: "new-input",
    session_end: "new-end",
    user_prompt_submit: "old-prompt",
    stop: "new-stop"
  }), false);
});
