import { defineStore } from "pinia";
import { ref } from "vue";
import type { ITheme } from "@xterm/xterm";
import { settingGet, settingSet } from "../ipc/client";
import { DEFAULT_THEME_NAME, findTheme } from "../components/terminal/themes";

export interface HighlightRule {
  /** Text or regex pattern. */
  pattern: string;
  isRegex: boolean;
  /** Background color applied to matches (css color, "" = none). */
  bg: string;
  /** Foreground color applied to matches (css color, "" = none). */
  fg: string;
}

const DEFAULT_RULES: HighlightRule[] = [
  { pattern: "\\b(error|err|failed|failure|fatal)\\b", isRegex: true, bg: "", fg: "#f14c4c" },
  { pattern: "\\b(warn|warning)\\b", isRegex: true, bg: "", fg: "#e5c07b" },
  { pattern: "\\b(success|succeeded|ok|done)\\b", isRegex: true, bg: "", fg: "#23d18b" },
];

/** Global terminal appearance settings, persisted in the settings table. */
export const useTerminalSettings = defineStore("terminalSettings", () => {
  const asciiFont = ref("Consolas");
  const cjkFont = ref("Microsoft YaHei");
  const fontSize = ref(14);
  const scrollback = ref(10000);
  const cursorStyle = ref<"block" | "underline" | "bar">("block");
  const cursorBlink = ref(true);
  const themeName = ref(DEFAULT_THEME_NAME);
  /** Custom theme imported from JSON; overrides themeName when set. */
  const customTheme = ref<ITheme | null>(null);
  const highlightRules = ref<HighlightRule[]>([...DEFAULT_RULES]);
  /** Paste throttling: chunk size in characters (0 = send whole), delay between chunks. */
  const pasteChunkSize = ref(1024);
  const pasteDelayMs = ref(30);
  /** Whole-window opacity (0.4 - 1). */
  const opacity = ref(1);
  const loaded = ref(false);

  function theme(): ITheme | undefined {
    return customTheme.value ?? findTheme(themeName.value);
  }

  async function load() {
    const [af, cf, fs, sb, cs, cb, th, hl, pc, pd, op] = await Promise.all([
      settingGet("term.asciiFont"),
      settingGet("term.cjkFont"),
      settingGet("term.fontSize"),
      settingGet("term.scrollback"),
      settingGet("term.cursorStyle"),
      settingGet("term.cursorBlink"),
      settingGet("term.theme"),
      settingGet("term.highlight"),
      settingGet("paste.chunkSize"),
      settingGet("paste.delayMs"),
      settingGet("ui.opacity"),
    ]);
    if (af) asciiFont.value = af;
    if (cf) cjkFont.value = cf;
    if (fs) fontSize.value = parseInt(fs, 10) || 14;
    if (sb) scrollback.value = Math.max(100, parseInt(sb, 10) || 10000);
    if (cs === "block" || cs === "underline" || cs === "bar") cursorStyle.value = cs;
    if (cb) cursorBlink.value = cb === "true";
    if (th) {
      try {
        const parsed = JSON.parse(th);
        if (parsed.custom) {
          customTheme.value = parsed.custom as ITheme;
        } else if (typeof parsed.name === "string") {
          themeName.value = parsed.name;
          customTheme.value = null;
        }
      } catch {
        /* ignore corrupt theme */
      }
    }
    if (hl) {
      try {
        const parsed = JSON.parse(hl);
        if (Array.isArray(parsed)) highlightRules.value = parsed;
      } catch {
        /* ignore corrupt rules */
      }
    }
    if (pc !== null) pasteChunkSize.value = Math.max(0, parseInt(pc, 10) || 0);
    if (pd !== null) pasteDelayMs.value = Math.max(0, parseInt(pd, 10) || 0);
    if (op !== null) {
      const v = parseFloat(op);
      if (v >= 0.4 && v <= 1) opacity.value = v;
    }
    loaded.value = true;
  }

  async function save() {
    await Promise.all([
      settingSet("term.asciiFont", asciiFont.value),
      settingSet("term.cjkFont", cjkFont.value),
      settingSet("term.fontSize", String(fontSize.value)),
      settingSet("term.scrollback", String(scrollback.value)),
      settingSet("term.cursorStyle", cursorStyle.value),
      settingSet("term.cursorBlink", String(cursorBlink.value)),
      settingSet(
        "term.theme",
        JSON.stringify(customTheme.value ? { custom: customTheme.value } : { name: themeName.value })
      ),
      settingSet("term.highlight", JSON.stringify(highlightRules.value)),
      settingSet("paste.chunkSize", String(pasteChunkSize.value)),
      settingSet("paste.delayMs", String(pasteDelayMs.value)),
      settingSet("ui.opacity", String(opacity.value)),
    ]);
  }

  function setTheme(name: string) {
    themeName.value = name;
    customTheme.value = null;
  }

  function setCustomTheme(t: ITheme) {
    customTheme.value = t;
  }

  return {
    asciiFont,
    cjkFont,
    fontSize,
    scrollback,
    cursorStyle,
    cursorBlink,
    themeName,
    customTheme,
    highlightRules,
    pasteChunkSize,
    pasteDelayMs,
    opacity,
    loaded,
    theme,
    load,
    save,
    setTheme,
    setCustomTheme,
  };
});
