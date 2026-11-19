<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import "@xterm/xterm/css/xterm.css";
import {
  b64decode,
  sessionInputBytes,
  sessionResize,
  type OutputEvent,
} from "../../ipc/client";
import { useTerminalSettings, type HighlightRule } from "../../stores/terminalSettings";
import { useAppStore } from "../../stores/app";
import { useMacroStore } from "../../stores/macros";

const props = defineProps<{ tabId?: string }>();

const emit = defineEmits<{
  (e: "state", state: string, message?: string): void;
}>();

const ts = useTerminalSettings();
const store = useAppStore();
const macroStore = useMacroStore();

const container = ref<HTMLDivElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon | null = null;
let search: SearchAddon | null = null;
let serializeAddon: SerializeAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
let sessionId: string | null = null;
let disposed = false;

const fontFamily = () => `${ts.asciiFont || "Consolas"}, ${ts.cjkFont || "Microsoft YaHei"}, monospace`;

function handleOutput(ev: OutputEvent) {
  if (!term) return;
  switch (ev.kind) {
    case "data":
      term.write(b64decode(ev.data));
      break;
    case "state":
      emit("state", ev.state, ev.message);
      break;
    case "exit":
      term.write(`\r\n[process exited${ev.code !== undefined ? ` code=${ev.code}` : ""}]\r\n`);
      break;
  }
}

/** Bind a backend session id to this terminal. */
function attach(id: string) {
  sessionId = id;
}

function detach() {
  sessionId = null;
}

// ---------- broadcast-aware input ----------

/** Sessions that should receive input typed into this terminal. */
function inputTargets(): string[] {
  if (!sessionId) return [];
  if (store.broadcastEnabled && props.tabId && !store.broadcastExcluded.has(props.tabId)) {
    const all = store.broadcastSessions();
    if (all.length) return all;
  }
  return [sessionId];
}

function sendBytes(bytes: Uint8Array) {
  for (const sid of inputTargets()) void sessionInputBytes(sid, bytes).catch(() => {});
}

/** Send text, split into throttled chunks when paste limits are configured. */
async function sendThrottled(text: string) {
  const chunk = ts.pasteChunkSize;
  const delay = ts.pasteDelayMs;
  const encoder = new TextEncoder();
  if (!chunk || chunk <= 0 || text.length <= chunk) {
    sendBytes(encoder.encode(text));
    return;
  }
  // Array.from splits by code point so multi-byte chars stay intact.
  const chars = Array.from(text);
  for (let i = 0; i < chars.length; i += chunk) {
    sendBytes(encoder.encode(chars.slice(i, i + chunk).join("")));
    if (delay > 0 && i + chunk < chars.length) {
      await new Promise((r) => setTimeout(r, delay));
    }
  }
}

function focus() {
  term?.focus();
}

// ---------- search bar ----------

const searchVisible = ref(false);
const searchText = ref("");
const searchCase = ref(false);
const searchRegex = ref(false);

function openSearch() {
  searchVisible.value = true;
}

function closeSearch() {
  searchVisible.value = false;
  search?.clearDecorations();
  term?.focus();
}

function searchOpts() {
  return {
    caseSensitive: searchCase.value,
    regex: searchRegex.value,
    incremental: true,
    decorations: {
      matchBackground: "#613214",
      matchOverviewRuler: "#e2c08d",
      activeMatchBackground: "#0e639c",
      activeMatchColorOverviewRuler: "#4fc3f7",
    },
  };
}

function findNext() {
  if (searchText.value) search?.findNext(searchText.value, searchOpts());
}
function findPrev() {
  if (searchText.value) search?.findPrevious(searchText.value, searchOpts());
}

watch([searchText, searchCase, searchRegex], () => {
  search?.clearDecorations();
  findNext();
});

// ---------- keyword highlight ----------

interface CompiledRule {
  re: RegExp;
  fg: string;
  bg: string;
}

let rules: CompiledRule[] = [];
const MAX_DECORATIONS = 4000;
const decorationQueue: { dispose(): void }[] = [];

function compileRules(list: HighlightRule[]): CompiledRule[] {
  const out: CompiledRule[] = [];
  for (const r of list) {
    if (!r.pattern || (!r.fg && !r.bg)) continue;
    try {
      out.push({ re: new RegExp(r.isRegex ? r.pattern : escapeRe(r.pattern), "gi"), fg: r.fg, bg: r.bg });
    } catch {
      /* skip invalid regex */
    }
  }
  return out;
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

watch(
  () => ts.highlightRules,
  (list) => {
    rules = compileRules(list);
  },
  { deep: true, immediate: true }
);

/** Scan the buffer line that just completed and decorate matches. */
function highlightLine() {
  const t = term;
  if (!t || !rules.length) return;
  const buf = t.buffer.active;
  if (buf.type === "alternate") return; // skip full-screen apps (vim/htop)
  const absY = buf.baseY + buf.cursorY - 1;
  const line = buf.getLine(absY);
  if (!line) return;
  const text = line.translateToString(true);
  if (!text) return;
  const markerY = absY - (buf.baseY + buf.cursorY);
  for (const rule of rules) {
    rule.re.lastIndex = 0;
    let m: RegExpExecArray | null;
    let guard = 0;
    while ((m = rule.re.exec(text)) && guard++ < 50) {
      const marker = t.registerMarker(markerY);
      if (!marker) break;
      const dec = t.registerDecoration({
        marker,
        x: m.index,
        width: Math.max(1, m[0].length),
        height: 1,
        backgroundColor: rule.bg || undefined,
        foregroundColor: rule.fg || undefined,
        layer: rule.fg ? "top" : "bottom",
      });
      if (dec) {
        dec.onDispose(() => marker.dispose());
        decorationQueue.push(dec);
        if (decorationQueue.length > MAX_DECORATIONS) {
          decorationQueue.shift()?.dispose();
        }
      } else {
        marker.dispose();
      }
      if (m.index === rule.re.lastIndex) rule.re.lastIndex++;
    }
  }
}

// ---------- context menu ----------

const menu = ref<{ x: number; y: number } | null>(null);

function onContextMenu(e: MouseEvent) {
  e.preventDefault();
  menu.value = { x: e.clientX, y: e.clientY };
  const close = () => {
    menu.value = null;
    window.removeEventListener("click", close);
  };
  window.addEventListener("click", close);
}

function menuCopy() {
  const s = term?.getSelection();
  if (s) void navigator.clipboard.writeText(s).catch(() => {});
}

async function menuPaste() {
  if (!sessionId) return;
  try {
    const text = await navigator.clipboard.readText();
    if (text) void sendThrottled(text);
  } catch {
    /* clipboard denied */
  }
}

function menuSelectAll() {
  term?.selectAll();
  menuCopy();
}

function menuClear() {
  term?.clear();
}

/** Flash-highlight every occurrence of `matched` for a few seconds (trigger action). */
function flashHighlight(matched: string, color?: string) {
  if (!term || !search || !matched) return;
  const c = color && /^#[0-9a-fA-F]{3,8}$/.test(color) ? color : "#b58900";
  search.findNext(matched, {
    caseSensitive: false,
    decorations: {
      matchBackground: c,
      matchOverviewRuler: c,
      activeMatchBackground: c,
      activeMatchColorOverviewRuler: c,
    },
  });
  window.setTimeout(() => search?.clearDecorations(), 4000);
}

/** Serialize the buffer (incl. scrollback) so another window can restore it. */
function serialize(): string {
  return serializeAddon?.serialize() ?? "";
}

/** Write raw terminal data (e.g. a serialized buffer restore). */
function writeRaw(data: string | Uint8Array) {
  term?.write(data);
}

defineExpose({ attach, detach, focus, handleOutput, openSearch, flashHighlight, serialize, writeRaw });

// ---------- lifecycle ----------

onMounted(() => {
  if (!container.value) return;
  term = new Terminal({
    fontFamily: fontFamily(),
    fontSize: ts.fontSize,
    cursorBlink: ts.cursorBlink,
    cursorStyle: ts.cursorStyle,
    scrollback: ts.scrollback,
    theme: ts.theme(),
  });
  fit = new FitAddon();
  search = new SearchAddon();
  serializeAddon = new SerializeAddon();
  term.loadAddon(fit);
  term.loadAddon(search);
  term.loadAddon(serializeAddon);
  term.open(container.value);
  try {
    term.loadAddon(new WebglAddon());
  } catch {
    /* fall back to canvas renderer */
  }
  fit.fit();

  term.onData((data) => {
    if (sessionId) sendBytes(new TextEncoder().encode(data));
    macroStore.feed(props.tabId, data);
  });

  term.onLineFeed(highlightLine);

  // Ctrl+F opens the search bar.
  term.attachCustomKeyEventHandler((e) => {
    if (e.type === "keydown" && e.ctrlKey && (e.key === "f" || e.key === "F")) {
      openSearch();
      return false;
    }
    return true;
  });

  // Copy on select.
  term.onSelectionChange(() => {
    const s = term?.getSelection();
    if (s) void navigator.clipboard.writeText(s).catch(() => {});
  });

  const sendResize = () => {
    if (sessionId && term) void sessionResize(sessionId, term.cols, term.rows);
  };

  resizeObserver = new ResizeObserver(() => {
    if (disposed) return;
    try {
      fit?.fit();
      sendResize();
    } catch {
      /* container hidden */
    }
  });
  resizeObserver.observe(container.value);
  sendResize();
});

onBeforeUnmount(() => {
  disposed = true;
  resizeObserver?.disconnect();
  decorationQueue.forEach((d) => d.dispose());
  decorationQueue.length = 0;
  term?.dispose();
  term = null;
});

// Live-apply appearance changes to this (and every) open terminal.
watch([() => ts.asciiFont, () => ts.cjkFont, () => ts.fontSize], () => {
  if (!term) return;
  term.options.fontFamily = fontFamily();
  term.options.fontSize = ts.fontSize;
  try {
    fit?.fit();
  } catch {
    /* hidden */
  }
});
watch(
  () => ts.theme(),
  (t) => {
    if (term && t) term.options.theme = t;
  }
);
watch([() => ts.cursorStyle, () => ts.cursorBlink, () => ts.scrollback], () => {
  if (!term) return;
  term.options.cursorStyle = ts.cursorStyle;
  term.options.cursorBlink = ts.cursorBlink;
  term.options.scrollback = ts.scrollback;
});

const menuStyle = computed(() =>
  menu.value ? { left: `${menu.value.x}px`, top: `${menu.value.y}px` } : {}
);
</script>

<template>
  <div class="wrap" @contextmenu="onContextMenu">
    <div ref="container" class="terminal-container"></div>

    <!-- search bar -->
    <div v-if="searchVisible" class="searchbar" @keydown.esc="closeSearch">
      <input
        v-model="searchText"
        placeholder="搜索回滚缓冲…"
        autofocus
        @keydown.enter="findNext"
        @keydown.shift.enter="findPrev"
      />
      <button :class="{ on: searchCase }" title="区分大小写" @click="searchCase = !searchCase">Aa</button>
      <button :class="{ on: searchRegex }" title="正则" @click="searchRegex = !searchRegex">.*</button>
      <button title="上一个" @click="findPrev">↑</button>
      <button title="下一个" @click="findNext">↓</button>
      <button title="关闭" @click="closeSearch">✕</button>
    </div>

    <!-- context menu -->
    <div v-if="menu" class="ctxmenu" :style="menuStyle">
      <div class="mi" @click="menuCopy">复制</div>
      <div class="mi" @click="menuPaste">粘贴</div>
      <div class="mi" @click="menuSelectAll">全选并复制</div>
      <div class="mi" @click="openSearch">搜索…</div>
      <div class="sep" />
      <div class="mi" @click="menuClear">清除终端</div>
    </div>
  </div>
</template>

<style scoped>
.wrap {
  position: relative;
  width: 100%;
  height: 100%;
}
.terminal-container {
  width: 100%;
  height: 100%;
  overflow: hidden;
  background: #1e1e1e;
}
.searchbar {
  position: absolute;
  top: 6px;
  right: 12px;
  display: flex;
  gap: 4px;
  background: #252526;
  border: 1px solid #3c3c3c;
  border-radius: 6px;
  padding: 4px;
  z-index: 20;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
}
.searchbar input {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 3px 8px;
  font-size: 12px;
  width: 200px;
  outline: none;
}
.searchbar button {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 8px;
  cursor: pointer;
  font-family: Consolas, monospace;
}
.searchbar button.on {
  background: #0e639c;
  color: #fff;
  border-color: #0e639c;
}
.ctxmenu {
  position: fixed;
  z-index: 200;
  background: #2d2d30;
  border: 1px solid #454545;
  border-radius: 6px;
  padding: 4px 0;
  min-width: 130px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
}
.ctxmenu .mi {
  padding: 5px 16px;
  font-size: 12px;
  color: #ddd;
  cursor: pointer;
}
.ctxmenu .mi:hover {
  background: #094771;
}
.ctxmenu .sep {
  height: 1px;
  background: #454545;
  margin: 4px 0;
}
</style>
