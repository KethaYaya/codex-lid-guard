# Codex Lid Guard

**Close your laptop lid. Let Codex keep working.**

Codex Lid Guard keeps your Windows laptop awake while local Codex tasks run, then restores your normal power settings when they finish. Optional desktop previews let you check progress from other apps, and background tasks can keep running after you close VS Code.

**Windows 10/11 x64 | VS Code 1.90+ | Official OpenAI Codex extension**

[Get started](#quick-start) · [Desktop previews](#message-overlays) · [Background tasks](#background-tasks) · [Shortcuts](#keyboard-shortcuts) · [Settings](#settings) · [Help](#troubleshooting)

![A project overlay with two sessions, one selected update, and a named project tab.](images/screenshots/overlay-project-groups.png)

*A native preview with sample sessions. Select a task to read its update; choose Open chat to return to the conversation.*

## What you can do

- **Keep working with the lid closed.** Protection starts automatically and stays on until the last active task finishes, including tasks in other VS Code windows.
- **Follow several projects from other apps.** Small two-line tabs show the project, highest-priority task, status icon, and extra-session count. Follow three projects by default, or choose up to ten.
- **Run tasks independently of VS Code.** Start a background task in Lid Guard and return to its own conversation window whenever you need to.
- **Return to normal sleep automatically.** Your original lid settings are restored when work stops. If the lid is still closed, the laptop sleeps after a 10-second grace period by default.

## Quick start

### 1. Install

In VS Code, install and enable the **official OpenAI Codex extension**, then sign in and open a local project folder.

You'll also need **`codex-lid-guard.vsix`**, the Lid Guard installer. Use a copy shared with you, or [build it from source](#build-from-source).

1. Press **Ctrl + Shift + P** to open the Command Palette.
2. Run **Extensions: Install from VSIX...** and select the file.
3. Reload VS Code if prompted. When updating, reload any other open VS Code windows too.

The installer includes the helper that runs in the background. You do not need Node.js, Rust, .NET, administrator access, or optional hook setup to use lid protection.

### 2. Start a Codex task

Use Codex in VS Code as usual. Lid Guard enables itself automatically.

Look for **Codex awake · 1** in the status bar before closing the lid. The number tells you how many tasks Lid Guard is keeping awake. When the last task stops, your saved lid settings are restored. Opening the lid or starting another task cancels pending sleep.

To keep a task running **after VS Code closes**, use [Start Background Task](#background-tasks) to create a conversation managed by Lid Guard.

### 3. Turn on desktop previews (optional)

Run **Codex Lid Guard: Toggle Message Overlay** from the Command Palette. Previews are **off by default**.

Wait for a new assistant update, then switch to another app or chat. A named project tab appears at the screen edge. Hover or click to see its sessions, select a task, and choose **Open chat**.

To try the controls without starting a task, run **Codex Lid Guard: Preview Message Overlay**. It shows five sample sessions across two projects (subject to your tab limit) and closes automatically after 35 seconds.

## Message overlays

Tabs and drawers use a dark glass style with soft highlights and fine edges. Adjust **Overlay Opacity** to control transparency. The glass lighting is static, so it adds no continuous animation or live background blur.

![Project tabs and a glass drawer with a selected session.](images/screenshots/overlay-project-groups.png)

Each project has a small 152 by 42 DIP tab showing its name, highest-priority task, status icon, and a +N count for other sessions. Expand it to see a 344 DIP drawer with up to four compact task rows, a two-line update, and Open/Dismiss actions. Larger lists scroll; selecting a task keeps the rows in place. Neighboring tabs visibly slide aside first; expansion waits until they are clear. Tabs slide back only after the drawer has fully folded. Switching projects folds the previous drawer first. The expanded project stays above sibling tabs without taking keyboard focus. Same-named folders get distinct path labels, with their full path shown in the drawer header.

The session currently viewed in a focused editor hides from the preview; other sessions in its project remain available. The project tab hides when it has no visible sessions. It returns when you switch away.

| To… | Do this |
| --- | --- |
| Peek at a project | Hover over its tab. Move away to fold the preview back. |
| Keep a project open | Click its tab. |
| Select a session | Click its task title. The selected row is highlighted and its update appears below the list. |
| Fold a project | Click the chevron in the header. |
| Open the full conversation | Choose **Open chat**, **Reply in chat**, or **Read result**. Double-clicking a task title also opens it. |
| Dismiss one session until its next task | Choose **Dismiss** beneath the selected update. Other sessions stay available; the task is not stopped. |
| Browse a larger project | Scroll inside the session list with the mouse wheel, or drag/click its scrollbar. The preview stays fixed below it. |

New messages update the preview without taking keyboard focus. Long replies are shortened; open the conversation to read them in full.

### Know when a task is done

| Indicator | Meaning |
| --- | --- |
| **Working** | The task is running. |
| **Needs you** | A reported approval or question is waiting for your response. |
| **Done · unread** / **unread result** | The task finished and you have not viewed its chat yet. |
| **Done** | The task completed and its result has been viewed. |
| **Idle** | No active work or confirmed completion is reported for this session. |

Reading a preview does not mark the completion as viewed. Open the chat to clear its indicator, or start a new task in that chat. Tasks sort by needs-response, working, unread completion, then idle/read completion; selection stays on the same task. A filled dot means working, a ringed exclamation means needs-response, and a check means done. The project stripe turns amber while any session needs you and otherwise uses its project color. Needs-response indicators are immediate for background sessions; editor sessions use the optional request hooks.

Choose **1–10 project tabs** with **Overlay Max Tabs** in Settings. At the limit, a more recently active project can replace an older one. Recent tabs remain available after VS Code closes while the helper is running, so you can reopen the original project and session. Retaining a tab alone does not keep Windows awake or keep an editor task running.

## Background tasks

Use this mode when you want a task to continue after **all VS Code windows are closed**.

1. Open a **trusted local project folder** in VS Code, with Codex installed and signed in.
2. Run **Codex Lid Guard: Start Background Task** from the Command Palette.
3. Describe the task. Lid Guard opens a separate conversation window.
4. Choose **Minimize to tab**, or close that conversation window. The task continues, and you can close VS Code too.
5. To return, open its overlay, run **Codex Lid Guard: Background Sessions**, or choose **Background Codex sessions…** from the Windows tray shield menu. The tray menu works even with overlays off.

In the conversation window, you can read replies, answer questions, review approval requests, stop the current task, and send follow-ups. Tasks can edit the selected project; additional permissions require your approval through **Allow once** or **Deny**. Sleep protection stays active while a task works or waits for your response.

**Closing the conversation window keeps the session running. Choosing End session stops it.** You can keep up to ten background sessions open.

This starts a new conversation; it cannot take over a task already running inside VS Code. Lid Guard and Windows must remain running. Quitting the helper or restarting Windows interrupts background work, and Lid Guard does not automatically resume it. Codex keeps the conversation in its normal local history. Unsupported interactions, such as secret input or MCP elicitation, return an error without granting permission.

## Keyboard shortcuts

These shortcuts work from other apps while overlay tabs are visible. The defaults use the physical **Copilot** key; you can choose another key combination in Settings.

| Shortcut | Action |
| --- | --- |
| Hold **Copilot**, tap **Tab** | Preview the next project. Tap Tab again to keep cycling. |
| **Enter** after selecting a project | Open its selected session. |
| **Esc** immediately after selecting a project | Dismiss its selected session until its next task. Other sessions stay available. |
| **Tab** after releasing Copilot | Fold the keyboard-selected preview immediately. |
| **Copilot + first shortcut letter**, then the **second letter** | Preview the project, then open its selected session. The assigned letters are always visible beside each project name and highlight while the prefix is held. |

Previews selected with **Copilot + Tab** fold automatically three seconds after you release Copilot. Click-opened and letter-selected previews stay open.

**Without a Copilot key**, set **Overlay Shortcut Prefix** to something like `Ctrl+Alt+Space`. Press that combination, then Tab to choose a project and Enter to open its selected session. Configure these global shortcuts in extension Settings.

<details>
<summary>Shortcut timing and customization</summary>

- The default Copilot binding expects **Win + Shift + F23**. Keyboard utilities that remap that sequence can prevent detection.
- Some keyboards send Copilot as a quick tap even when you hold it. On those keyboards, enter each next step within **1.5 seconds**. Press Copilot again before Tab to keep cycling; Tab alone folds the selected preview.
- Enter and Esc return to their normal behavior when the shortcut expires or focus changes. Esc before selecting a tab cancels the shortcut.
- Customize **Overlay Cycle Key**, **Overlay Open Key**, and **Overlay Close Key** using different keys for each. Shortcut letters stay visible on tabs whenever shortcuts are enabled; holding the prefix highlights them.
- Changes apply immediately. Invalid or duplicate bindings disable shortcuts and show a settings warning.
- Turn off **Overlay Shortcuts Enabled** to disable global shortcuts. Without visible tabs, Copilot keeps its normal Windows behavior. These bindings are separate from VS Code's Keyboard Shortcuts editor.

</details>

## Status, sounds, and quitting

Click the shield in VS Code's status bar, or run **Codex Lid Guard: Show Status**, to see running and recent chats. Select a chat to open it. Completed chats stay highlighted until viewed.

| Status | Meaning |
| --- | --- |
| **Codex Lid Guard** with a shield | Ready; no tasks currently need protection. |
| **Codex awake · N** | Keeping Windows awake for `N` active tasks. |
| **Codex sleep pending** | The lid is closed and the sleep grace period is running. |
| **Lid Guard stopped**, or a disabled icon | Click to enable Lid Guard again. |
| A warning or error icon | Hover over it for details. |

Completion sounds are on by default and stay quiet while you view the relevant chat. Run **Codex Lid Guard: Test Alert Sounds** to hear the samples. For immediate approval and question alerts, run **Codex Lid Guard: Enable Optional Hook Alerts** and complete the one-time Codex hook review. Core lid protection works without this review.

To quit completely, click or right-click the shield in the **Windows notification area** and choose **Quit Lid Guard (close all tabs)**. This closes overlays, ends background workers, cancels pending sleep, and restores saved power settings. Run **Codex Lid Guard: Enable** to start it again.

## Settings

Open VS Code Settings with **Ctrl + ,** and search for **Codex Lid Guard**.

| If you want to… | Change this setting |
| --- | --- |
| See desktop chat previews | Turn on **Message Overlay**. Default: off. |
| Follow more projects | Set **Overlay Max Tabs** from 1 to 10. Default: 3. |
| Move the previews | Choose a corner with **Overlay Position**. Default: bottom-right. |
| Make previews more transparent | Lower **Overlay Opacity**. Default: 82%. |
| Use a keyboard without a Copilot key | Change **Overlay Shortcut Prefix**, for example to `Ctrl+Alt+Space`. |
| Turn off sounds | Turn off **Alert Sounds**. Default: on. |
| Allow more time before sleep | Increase **Sleep Delay Seconds**. Default: 10 seconds. |
| Disable Lid Guard's automatic sleep after tasks | Turn off **Sleep When Lid Closed**. Your original Windows lid settings still get restored. |

<details>
<summary>All settings and defaults</summary>

All keys below start with `codexLidGuard.`. Use the full key when editing `settings.json`.

| Setting | Default | Options or behavior |
| --- | --- | --- |
| `enabled` | `true` | Monitor and protect local tasks automatically. |
| `messageOverlay` | `false` | Show assistant previews for background chats. |
| `overlayMaxTabs` | `3` | 1–10 project tabs. Each groups its sessions; one project expands at a time. |
| `overlayOpacity` | `82` | 30–100 percent. |
| `overlayPosition` | `bottom-right` | `bottom-right`, `bottom-left`, `top-right`, or `top-left`. |
| `overlayDurationSeconds` | `90` | 10–600 seconds for older previews. Recent chats and unread completions remain available, subject to the tab limit. |
| `overlayShortcutsEnabled` | `true` | Enable global overlay shortcuts. |
| `overlayShortcutPrefix` | `Copilot` | `Copilot`, or a combination with Ctrl, Alt, or Win plus one key. |
| `overlayCycleKey` | `Tab` | Select the next tab after the prefix. |
| `overlayOpenKey` | `Enter` | Open the selected chat. Its second tab letter also works. |
| `overlayCloseKey` | `Escape` | Dismiss the selected session or cancel the shortcut. |
| `alertSounds` | `true` | Play completion and request sounds. |
| `alertSoundsOnlyWhenUnfocused` | `true` | Keep automatic alerts quiet while viewing the relevant chat. |
| `optionalHooks` | `false` | Enable request-alert hooks after a one-time Codex review. |
| `sleepWhenLidClosed` | `true` | Sleep after the last task stops if the lid is still closed. |
| `sleepDelaySeconds` | `10` | 0–300 seconds before sleep. Opening the lid or starting another task cancels it. |

For example, add these to your VS Code `settings.json` to enable previews and allow 30 seconds before sleep:

```json
{
  "codexLidGuard.messageOverlay": true,
  "codexLidGuard.sleepDelaySeconds": 30
}
```

</details>

<details>
<summary>All Command Palette commands</summary>

Press **Ctrl + Shift + P** and search for **Codex Lid Guard**. Every command below has the prefix **Codex Lid Guard:**.

| Command | What it does |
| --- | --- |
| **Enable** | Start automatic protection. |
| **Disable and Restore Power Settings** | Disable monitoring, restore saved settings, and remove optional Lid Guard hooks. |
| **Show Status** | Show status or the running and recent session menu. |
| **Start Background Task** | Start a conversation that can continue after VS Code closes. |
| **Background Sessions** | Open a background conversation window. |
| **Restore Power Settings Now** | Restore saved Windows power settings immediately. |
| **Toggle Message Overlay** | Turn desktop previews on or off. |
| **Preview Message Overlay** | Try the 35-second sample chat demo. |
| **Enable Optional Hook Alerts** | Set up immediate approval and question alerts. |
| **Test Alert Sounds** | Play both alert samples when sounds are enabled. |

</details>

## Troubleshooting

| Problem | What to try |
| --- | --- |
| **No desktop tab appears** | Turn on **Toggle Message Overlay**, wait for a new assistant update, and switch away from that chat. Run **Preview Message Overlay** to check that previews display. |
| **A tab disappeared** | Its sessions may be focused, dismissed, or outside the recent-project limit. Increase **Overlay Max Tabs** if needed. |
| **Copilot opens another app** | Make sure a tab is visible. Check keyboard remapping, or set **Overlay Shortcut Prefix** to `Ctrl+Alt+Space`. |
| **Enter or Esc does nothing** | Select a tab with the shortcut first, then press Enter or Esc promptly. On quick-tap Copilot keyboards, each next step must be within 1.5 seconds. |
| **The right chat does not open** | Reload existing VS Code windows after an update. Check **View → Output → Codex Lid Guard Navigation** for details. |
| **A background task will not start** | Open a trusted local Windows project, enable Lid Guard, and check that Codex is installed and signed in. Remote workspaces are not supported for background tasks. |
| **There is no alert sound** | Check **Alert Sounds**. The chat you are viewing is quiet by default. Approval and question alerts also need optional hooks and their review. |
| **Windows stays awake after a task finishes** | Click the shield to check for another active task, including one waiting for your response. Use **Restore Power Settings Now** to restore the saved policy. |

Lid Guard protects **local Windows tasks**. Codex cloud tasks run elsewhere. Compatibility with local Codex session formats can change when Codex updates.

Still stuck? [Open an issue](https://github.com/KethaYaya/codex-lid-guard/issues) with your Windows, VS Code, Codex, and Lid Guard versions, plus the steps to reproduce the problem.

## Privacy and uninstalling

Core protection reads local Codex lifecycle information to detect running tasks. It does not query prompt or response fields from the metadata database.

Turning on overlays also reads new assistant display messages from local session files; user prompts, reasoning, and tool output are excluded from previews. Preview text stays in memory, is excluded from Lid Guard logs and status snapshots, and is cleared when you turn overlays off. Background tasks use the installed Codex runtime and your existing sign-in; Codex handles their conversation history as usual.

Lid Guard saves your battery and plugged-in lid settings before changing them. A recovery record at `%LOCALAPPDATA%\CodexLidGuard\power-recovery.json` lets its next launch restore those settings after an interruption.

**Before uninstalling**, run **Codex Lid Guard: Disable and Restore Power Settings** to restore power settings and remove optional hook entries. To end background sessions and close all tabs as well, use **Quit Lid Guard (close all tabs)** from the Windows tray shield menu.

## Build from source

If you already have a VSIX file, use the [installation steps above](#1-install). To create the installer yourself, build on Windows with **Git**, **Node.js 20+**, **Rust through `rustup`**, and the **Visual Studio C++ build tools** installed:

```powershell
git clone https://github.com/KethaYaya/codex-lid-guard.git
cd codex-lid-guard\extension
npm.cmd ci
npm.cmd run package
```

This compiles the extension and native helper and creates **`extension/codex-lid-guard.vsix`**. Install it through **Extensions: Install from VSIX...**, or, from the `extension` folder:

```powershell
code --install-extension .\codex-lid-guard.vsix
```

<details>
<summary>Developer checks and diagnostics</summary>

From the repository root, after installing dependencies with `npm.cmd ci` in `extension`:

```powershell
npm.cmd --prefix .\extension test
cargo test --manifest-path .\native\CodexLidGuard\Cargo.toml --locked
cargo clippy --manifest-path .\native\CodexLidGuard\Cargo.toml --all-targets --locked -- -D warnings
```

Native tests that display preview windows are ignored by default and run separately for interaction checks.

- **Helper diagnostics:** `status`, `restore`, `overlay-preview`, `sound done`, and `sound request`.
- **Logs:** `%LOCALAPPDATA%\CodexLidGuard\guard.log`, rotated at 1 MB.
- **Current state:** `%LOCALAPPDATA%\CodexLidGuard\status.json`.
- **Optional hook backup:** `~/.codex/hooks.json.before-codex-lid-guard`.

</details>

## License and credits

[MIT licensed](LICENSE). See the [changelog](CHANGELOG.md) for updates. Screenshots show the native demo with sample chat text. Bundled sounds are from Herdr 0.8.2 under Apache-2.0; see the [third-party notices](THIRD_PARTY_NOTICES.md).
