import type { OverlayShortcuts } from "./helper";

const actionKey = /^(?:Tab|Enter|Escape|Space|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Up|Down|Left|Right|F(?:[1-9]|1[0-9]|2[0-4]))$/iu;

export function overlayShortcutProblem(settings: OverlayShortcuts): string | undefined {
  if (!settings.enabled) { return undefined; }
  const keys = [settings.cycleKey, settings.openKey, settings.closeKey].map((key) => key.trim());
  if (keys.some((key) => !actionKey.test(key))) { return "Choose a supported cycle, open and close key."; }
  if (new Set(keys.map((key) => key.toLowerCase())).size !== 3) {
    return "The cycle, open and close keys must be different.";
  }
  const prefix = settings.prefix.trim();
  if (prefix.toLowerCase() === "copilot") { return undefined; }
  const parts = prefix.split("+").map((part) => part.trim());
  const trigger = parts.pop() ?? "";
  const modifiers = parts.map((part) => part.toLowerCase());
  if (!modifiers.some((part) => ["ctrl", "alt", "win"].includes(part))
      || modifiers.some((part) => !["ctrl", "alt", "shift", "win"].includes(part))
      || new Set(modifiers).size !== modifiers.length
      || !(actionKey.test(trigger) || /^[a-z0-9]$/iu.test(trigger))) {
    return "Use Copilot or a prefix such as Ctrl+Alt+Space. Custom prefixes need Ctrl, Alt or Win and one key.";
  }
  return undefined;
}
