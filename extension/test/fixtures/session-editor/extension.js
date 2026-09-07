exports.activate = () => {
  const vscode = require('vscode');
  // Extension-test mode uses in-memory VS Code storage. The restore regression
  // runs as a normal development extension so the two processes share a layout.
  if (!process.env.LID_GUARD_RESTORE_TEST_MODULE) { return { vscode }; }
  // URI delivery waits for extension activation. Run the test after activation
  // returns so opening a URI cannot wait on the test that sent it.
  setImmediate(async () => {
  const fs = require('node:fs/promises');
  const path = require('node:path');
  const result = path.join(process.env.LID_GUARD_RESTORE_RUN, `${process.env.LID_GUARD_RESTORE_PHASE}.json`);
  try {
    await require(process.env.LID_GUARD_RESTORE_TEST_MODULE).run();
    await fs.writeFile(result, JSON.stringify({ passed: true }));
  } catch (error) {
    await fs.writeFile(result, JSON.stringify({ passed: false, error: error.stack }));
  }
  await vscode.commands.executeCommand('workbench.action.closeWindow');
  });
  return { vscode };
};
