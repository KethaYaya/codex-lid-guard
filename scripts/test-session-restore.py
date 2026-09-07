"""Test an actual close/reopen using a disposable VS Code profile and workspace."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--code", required=True, type=Path)
parser.add_argument("--maximized-sidebar", action="store_true")
parser.add_argument("--codex-extension", type=Path)
parser.add_argument("--session", help="Existing local chat for the optional real Codex smoke test")
args = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix="codex-lid-guard-restore-"))
workspace = run / "workspace"
workspace.mkdir()
settings = run / "profile/User/settings.json"
settings.parent.mkdir(parents=True)
settings.write_text(json.dumps({
    "workbench.startupEditor": "none", "update.mode": "none",
    "extensions.autoUpdate": False, "extensions.autoCheckUpdates": False,
    "telemetry.telemetryLevel": "off", "window.restoreWindows": "all",
    "extensions.confirmedUriHandlerExtensionIds": ["openai.chatgpt"]
}))
environment = dict(os.environ)
environment["LID_GUARD_RESTORE_RUN"] = str(run)
environment["LID_GUARD_RESTORE_TEST_MODULE"] = str(repo / "extension/dist/test/integration/sessionRestore.js")
if args.maximized_sidebar:
    environment["LID_GUARD_RESTORE_MAXIMIZED"] = "1"
development = repo / "extension/test/fixtures/session-editor"
development_paths = ["--extensionDevelopmentPath=" + str(development)]
if args.codex_extension:
    assert args.session
    fixture = run / "fixture"
    fixture.mkdir()
    (fixture / "extension.js").write_bytes((development / "extension.js").read_bytes())
    (fixture / "package.json").write_text(json.dumps({
        "name": "restore-smoke-test", "publisher": "codex-lid-guard-tests", "version": "0.0.1",
        "engines": {"vscode": "^1.90.0"}, "main": "./extension.js", "activationEvents": ["onStartupFinished"]
    }))
    development_paths = ["--extensionDevelopmentPath=" + str(fixture),
                         "--extensionDevelopmentPath=" + str(args.codex_extension.resolve())]
    environment["LID_GUARD_RESTORE_TEST_MODULE"] = str(repo / "extension/dist/test/integration/realSessionRestore.js")
    environment["LID_GUARD_REAL_SESSION"] = args.session
for key in ["ELECTRON_RUN_AS_NODE", "VSCODE_IPC_HOOK_CLI", "VSCODE_CLI"]:
    environment.pop(key, None)
for phase in ["seed", "restore"]:
    environment["LID_GUARD_RESTORE_PHASE"] = phase
    log = run / f"{phase}.log"
    with log.open("w", encoding="utf-8") as output:
        result = subprocess.run([
            str(args.code.resolve()), str(workspace),
            "--user-data-dir", str(run / "profile"), "--extensions-dir", str(run / "extensions"),
            *development_paths,
            "--skip-welcome", "--skip-release-notes", "--disable-workspace-trust", "--disable-extensions"
        ], env=environment, stdout=output, stderr=subprocess.STDOUT,
            creationflags=subprocess.CREATE_NO_WINDOW, timeout=90)
    text = log.read_text(encoding="utf-8", errors="replace")
    outcome_file = run / f"{phase}.json"
    outcome = json.loads(outcome_file.read_text()) if outcome_file.exists() else {"passed": False}
    passed = result.returncode == 0 and outcome["passed"]
    if args.maximized_sidebar and passed:
        databases = list((run / "profile/User/workspaceStorage").glob("*/state.vscdb"))
        assert len(databases) == 1
        with sqlite3.connect(f"file:{databases[0]}?mode=ro", uri=True) as connection:
            hidden = connection.execute("select value from ItemTable where key='workbench.editor.hidden'").fetchone()
        expected = "true"  # Sidebar navigation must preserve a maximized sidebar.
        passed = hidden == (expected,)
        if not passed:
            outcome = {"passed": False, "error": f"editor.hidden={hidden}; expected {expected}"}
    print(f"PASS: {phase}" if passed else json.dumps(outcome) + "\n" + text[-4000:])
    print(json.dumps({"phase": phase, "exitCode": result.returncode, "log": str(log)}))
    if not passed:
        raise SystemExit(result.returncode or 1)
