import { defineStore } from "pinia";
import { ref } from "vue";
import { sessionInputText, settingGet, settingSet } from "../ipc/client";
import { useAppStore } from "./app";

export interface MacroStep {
  /** Milliseconds to wait before sending this chunk. */
  delay: number;
  data: string;
}

export interface Macro {
  id: string;
  name: string;
  steps: MacroStep[];
}

/** A global keybinding that sends text or replays a macro to the active session. */
export interface Shortcut {
  id: string;
  /** Combo string like "Ctrl+Shift+1" / "F5". */
  combo: string;
  /** Text to send (\n becomes Enter); ignored when macroId set. */
  text: string;
  /** Macro to replay; takes precedence over text. */
  macroId: string;
}

const MAX_STEPS = 2000;
const MAX_DELAY = 5000;

/** Keyboard macros + custom shortcuts, persisted in the settings table. */
export const useMacroStore = defineStore("macros", () => {
  const macros = ref<Macro[]>([]);
  const shortcuts = ref<Shortcut[]>([]);
  const loaded = ref(false);

  // ---------- recording ----------

  const recording = ref(false);
  const recordName = ref("");
  const recordSteps = ref(0);
  let buf: MacroStep[] = [];
  let lastTs = 0;

  function startRecord(name: string) {
    recordName.value = name;
    buf = [];
    lastTs = 0;
    recordSteps.value = 0;
    recording.value = true;
  }

  /** Called from TerminalView.onData for the active tab while recording. */
  function feed(tabId: string | undefined, data: string) {
    if (!recording.value) return;
    const app = useAppStore();
    if (!tabId || app.activeTabId !== tabId) return;
    const now = Date.now();
    const delay = lastTs ? Math.min(now - lastTs, MAX_DELAY) : 0;
    lastTs = now;
    if (buf.length < MAX_STEPS) {
      buf.push({ delay, data });
      recordSteps.value = buf.length;
    }
  }

  async function stopRecord(): Promise<void> {
    recording.value = false;
    if (buf.length && recordName.value.trim()) {
      macros.value.push({
        id: crypto.randomUUID(),
        name: recordName.value.trim(),
        steps: buf,
      });
      await save().catch(() => {});
    }
    buf = [];
    recordSteps.value = 0;
  }

  // ---------- replay ----------

  async function play(m: Macro, sessionId: string) {
    for (const s of m.steps) {
      if (s.delay > 0) {
        await new Promise((r) => setTimeout(r, Math.min(s.delay, 3000)));
      }
      await sessionInputText(sessionId, s.data);
    }
  }

  function remove(id: string) {
    macros.value = macros.value.filter((m) => m.id !== id);
    // drop shortcuts bound to the deleted macro
    shortcuts.value = shortcuts.value.filter((s) => s.macroId !== id);
    void save().catch(() => {});
  }

  // ---------- shortcuts ----------

  function upsertShortcut(sc: Shortcut) {
    const i = shortcuts.value.findIndex((s) => s.id === sc.id);
    if (i >= 0) shortcuts.value[i] = sc;
    else shortcuts.value.push(sc);
    void save().catch(() => {});
  }

  function removeShortcut(id: string) {
    shortcuts.value = shortcuts.value.filter((s) => s.id !== id);
    void save().catch(() => {});
  }

  /** Key event → "Ctrl+Shift+1" style combo; "" when no useful combo. */
  function comboOf(e: KeyboardEvent): string {
    if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return "";
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
    return parts.length >= 2 || /^F\d{1,2}$/.test(e.key) ? parts.join("+") : "";
  }

  /** Fire the shortcut bound to this key event, if any. Returns true when handled. */
  async function fireShortcut(combo: string, sessionId: string): Promise<boolean> {
    const sc = shortcuts.value.find((s) => s.combo === combo);
    if (!sc) return false;
    const macro = sc.macroId ? macros.value.find((m) => m.id === sc.macroId) : null;
    if (macro) {
      await play(macro, sessionId);
    } else if (sc.text) {
      await sessionInputText(sessionId, sc.text.replace(/\r?\n/g, "\r"));
    } else {
      return false;
    }
    return true;
  }

  // ---------- persistence ----------

  async function load() {
    const raw = await settingGet("keybind.macros");
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed.macros)) macros.value = parsed.macros;
        if (Array.isArray(parsed.shortcuts)) shortcuts.value = parsed.shortcuts;
      } catch {
        /* ignore corrupt payload */
      }
    }
    loaded.value = true;
  }

  async function save() {
    await settingSet(
      "keybind.macros",
      JSON.stringify({ macros: macros.value, shortcuts: shortcuts.value })
    );
  }

  return {
    macros,
    shortcuts,
    loaded,
    recording,
    recordSteps,
    startRecord,
    feed,
    stopRecord,
    play,
    remove,
    upsertShortcut,
    removeShortcut,
    comboOf,
    fireShortcut,
    load,
    save,
  };
});
