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
    "telemetry.telemetryLevel": "off"
}))
environment = dict(os.environ)
for key in ["ELECTRON_RUN_AS_NODE", "VSCODE_IPC_HOOK_CLI", "VSCODE_CLI"]:
    environment.pop(key, None)
log = run / "test.log"
with log.open("w", encoding="utf-8") as output:
    result = subprocess.run([
        str(args.code.resolve()), str(workspace),
        "--user-data-dir", str(run / "profile"),
        "--extensions-dir", str(run / "extensions"),
        "--extensionDevelopmentPath=" + str(repo / "extension/test/fixtures/session-editor"),
        "--extensionTestsPath=" + str(repo / "extension/dist/test/integration/sessionEditor.js"),
        "--skip-welcome", "--skip-release-notes", "--disable-workspace-trust",
        "--disable-extensions"
    ], env=environment, stdout=output, stderr=subprocess.STDOUT,
        creationflags=subprocess.CREATE_NO_WINDOW, timeout=60)
text = log.read_text(encoding="utf-8", errors="replace")
passed = result.returncode == 0 and "PASS: exact chat navigation" in text
print("\n".join(line for line in text.splitlines() if line.startswith("PASS:"))
      if passed else text[-8000:])
print(json.dumps({"exitCode": result.returncode, "log": str(log)}))
raise SystemExit(0 if passed else result.returncode or 1)
