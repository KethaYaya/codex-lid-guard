"""Exercise the real VS Code tab API in a disposable extension test window.

Usage: python scripts/test-session-navigation.py --code <absolute Code.exe path>
Run `npm run compile` in extension first. No live guardian or chat is restarted.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--code", required=True, type=Path)
parser.add_argument("--helper", type=Path, help="Built native helper for end-to-end pipe navigation")
args = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix="codex-lid-guard-navigation-"))
workspace = run / "workspace"
workspace.mkdir()
settings = run / "profile/User/settings.json"
settings.parent.mkdir(parents=True)
settings.write_text(json.dumps({
    "workbench.startupEditor": "none",
    "update.mode": "none",
    "extensions.autoUpdate": False,
    "extensions.autoCheckUpdates": False,
    "telemetry.telemetryLevel": "off",
    "extensions.confirmedUriHandlerExtensionIds": ["openai.chatgpt"]
}))
environment = dict(os.environ)
environment["LOCALAPPDATA"] = str(run / "local")
environment["LID_GUARD_RESTORE_RUN"] = str(run)
environment["LID_GUARD_RESTORE_PHASE"] = "navigation"
environment["LID_GUARD_RESTORE_TEST_MODULE"] = str(repo / "extension/dist/test/integration/sessionSidebar.js")
if args.helper:
    environment["LID_GUARD_TEST_HELPER"] = str(args.helper.resolve())
    # The navigation test writes only to this disposable helper data directory.
    environment["LOCALAPPDATA"] = str(run / "local")
for key in ["ELECTRON_RUN_AS_NODE", "VSCODE_IPC_HOOK_CLI", "VSCODE_CLI"]:
    environment.pop(key, None)
log = run / "test.log"
with log.open("w", encoding="utf-8") as output:
    result = subprocess.run([
        str(args.code.resolve()), str(workspace),
        "--user-data-dir", str(run / "profile"),
        "--extensions-dir", str(run / "extensions"),
        "--extensionDevelopmentPath=" + str(repo / "extension/test/fixtures/session-editor"),
        "--skip-welcome", "--skip-release-notes", "--disable-workspace-trust",
        "--disable-extensions"
    ], env=environment, stdout=output, stderr=subprocess.STDOUT,
        creationflags=subprocess.CREATE_NO_WINDOW, timeout=60)
text = log.read_text(encoding="utf-8", errors="replace")
outcome_file = run / "navigation.json"
outcome = json.loads(outcome_file.read_text()) if outcome_file.exists() else {"passed": False}
passed = result.returncode == 0 and outcome["passed"]
print("PASS: sidebar chat navigation, navigation pipe, unchanged editor tabs, existing sidebar reuse, stale workspace rejection"
      if passed else json.dumps(outcome) + "\n" + text[-4000:])
print(json.dumps({"exitCode": result.returncode, "log": str(log)}))
raise SystemExit(0 if passed else result.returncode or 1)
