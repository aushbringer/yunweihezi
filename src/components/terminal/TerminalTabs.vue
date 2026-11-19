<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useAppStore, type Tab } from "../../stores/app";
import { sysNotify } from "../../ipc/notify";
import TerminalView from "./TerminalView.vue";
import HexViewer from "./HexViewer.vue";
import ComposePane from "./ComposePane.vue";
import {
  b64decode,
  serialXrecv,
  serialXsend,
  serialZsend,
  type OutputEvent,
} from "../../ipc/client";

const store = useAppStore();
const termRefs = ref<Record<string, InstanceType<typeof TerminalView> | null>>({});
const hexRefs = ref<Record<string, InstanceType<typeof HexViewer> | null>>({});
/** Per-tab hex view toggle. */
const hexOn = ref<Record<string, boolean>>({});
/** Compose pane (multi-line draft) visibility. */
const composeOn = ref(false);

function setRef(tabId: string) {
  return (el: any) => {
    termRefs.value[tabId] = el ?? null;
    if (el) {
      store.registerOutput(tabId, (ev: OutputEvent) => {
        if (ev.kind === "modem") {
          // Remote `rz` waits for files → prompt user to pick what to send.
          if (ev.event === "zrinit") void pickAndZsend(tabId);
          return;
        }
        if (ev.kind === "trigger") {
          handleTriggerEvent(tabId, ev);
          return;
        }
        el.handleOutput(ev);
        if (ev.kind === "data" && hexOn.value[tabId]) {
          hexRefs.value[tabId]?.append(b64decode(ev.data));
        }
      });
    } else {
      store.unregisterOutput(tabId);
    }
  };
}

// ---------- trigger feedback (highlight flash / alert toast) ----------

const toasts = ref<{ id: number; text: string }[]>([]);
let toastSeq = 0;

function pushToast(text: string) {
  const id = ++toastSeq;
  toasts.value.push({ id, text });
  window.setTimeout(() => {
    toasts.value = toasts.value.filter((t) => t.id !== id);
  }, 5000);
}

function handleTriggerEvent(tabId: string, ev: Extract<OutputEvent, { kind: "trigger" }>) {
  if (ev.action === "alert") {
    const body = (ev.param || "{match}").replace(/\{match\}/g, ev.matched);
    pushToast(`触发器「${ev.name}」: ${body}`);
    void sysNotify(`触发器「${ev.name}」`, body);
  } else if (ev.action === "highlight") {
    termRefs.value[tabId]?.flashHighlight(ev.matched, ev.param || undefined);
  }
}

// ---------- detach tab into its own window ----------

async function detachTab(tab: Tab) {
  if (!tab.sessionId) return;
  const serialized = termRefs.value[tab.id]?.serialize() ?? "";
  const sid = tab.sessionId;
  // Hand the current buffer over via shared localStorage (same origin).
  localStorage.setItem(
    `detach:${sid}`,
    JSON.stringify({ title: tab.title, kind: tab.kind, serialized })
  );
  const win = new WebviewWindow(`detached-${sid}`, {
    url: `index.html?detach=${sid}`,
    title: `${tab.title} — OpenTerm`,
    width: 960,
    height: 640,
  });
  win.once("tauri://error", (e) => {
    console.error("detached window failed:", e);
    localStorage.removeItem(`detach:${sid}`);
  });
  // Remove the tab here; the detached window re-attaches the backend session.
  store.detachTabLocal(tab.id);
}

function setHexRef(tabId: string) {
  return (el: any) => {
    hexRefs.value[tabId] = el ?? null;
  };
}

function toggleHex(tabId: string) {
  hexOn.value[tabId] = !hexOn.value[tabId];
  if (hexOn.value[tabId]) hexRefs.value[tabId]?.flushTail();
}

async function pickAndZsend(tabId: string) {
  const tab = store.tabs.find((t) => t.id === tabId);
  if (!tab?.sessionId) return;
  const picked = await openDialog({ multiple: true, directory: false });
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  if (paths.length > 0) {
    await serialZsend(tab.sessionId, paths).catch(() => {});
  }
}

async function sendModem(tabId: string, variant: "XModem" | "YModem") {
  const tab = store.tabs.find((t) => t.id === tabId);
  if (!tab?.sessionId) return;
  const picked = await openDialog({ multiple: false, directory: false });
  if (typeof picked === "string") {
    await serialXsend(tab.sessionId, picked, variant).catch(() => {});
  }
}

async function recvModem(tabId: string, variant: "XModem" | "YModem") {
  const tab = store.tabs.find((t) => t.id === tabId);
  if (!tab?.sessionId) return;
  await serialXrecv(tab.sessionId, variant).catch(() => {});
}

const activeTab = computed(() => store.tabs.find((t) => t.id === store.activeTabId));
const activeModem = computed(() =>
  store.activeTabId ? store.modemStatus.get(store.activeTabId) : undefined
);

function modemPercent(m: { transferred: number; total: number }) {
  if (!m.total) return "";
  return ` ${Math.min(100, Math.round((m.transferred / m.total) * 100))}%`;
}

// Attach the backend session id to the xterm instance once connected.
watch(
  () => store.tabs.map((t) => `${t.id}:${t.sessionId}`).join(","),
  async () => {
    await nextTick();
    for (const tab of store.tabs) {
      const inst = termRefs.value[tab.id];
      if (inst && tab.sessionId) inst.attach(tab.sessionId);
      if (inst && tab.id === store.activeTabId) inst.focus();
    }
  },
  { immediate: true }
);

async function closeTab(tabId: string, state: string) {
  if (state === "connected" || state === "connecting") {
    if (!confirm("会话仍在连接中,确定关闭?")) return;
  }
  await store.closeTab(tabId);
}

function stateClass(state: string) {
  switch (state) {
    case "connected":
      return "ok";
    case "connecting":
      return "busy";
    case "error":
    case "disconnected":
      return "err";
    default:
      return "idle";
  }
}
</script>

<template>
  <div class="tabs-wrap">
    <div class="tabbar">
      <div
        v-for="tab in store.tabs"
        :key="tab.id"
        class="tab"
        :class="{
          active: tab.id === store.activeTabId,
          bcout: store.broadcastEnabled && store.broadcastExcluded.has(tab.id),
        }"
        @click="store.activeTabId = tab.id"
      >
        <span class="dot" :class="stateClass(tab.state)" />
        <span class="title">
          <span v-if="tab.kind !== 'ssh'" class="kind">{{ tab.kind }}</span>
          {{ tab.title }}
        </span>
        <a
          v-if="store.broadcastEnabled"
          class="bc"
          :class="{ off: store.broadcastExcluded.has(tab.id) }"
          :title="store.broadcastExcluded.has(tab.id) ? '已排除广播,点击恢复' : '接收广播输入,点击排除'"
          @click.stop="store.toggleBroadcastExclude(tab.id)"
          >⇄</a
        >
        <a
          v-if="tab.sessionId"
          class="detach"
          title="分离为独立窗口"
          @click.stop="detachTab(tab)"
          >⧉</a
        >
        <a class="x" @click.stop="closeTab(tab.id, tab.state)">✕</a>
      </div>
      <div v-if="!store.tabs.length" class="placeholder">
        双击左侧会话建立连接
      </div>
      <span class="tabbar-spacer" />
      <button
        class="tabtool"
        :class="{ on: store.broadcastEnabled }"
        title="广播输入:键入内容同时发送到所有未排除的会话"
        @click="store.setBroadcast(!store.broadcastEnabled)"
      >
        广播
      </button>
      <button
        class="tabtool"
        :class="{ on: composeOn }"
        title="撰写窗格:多行命令编辑后发送"
        @click="composeOn = !composeOn"
      >
        撰写
      </button>
    </div>
    <div class="tab-body">
      <div
        v-for="tab in store.tabs"
        :key="tab.id"
        v-show="tab.id === store.activeTabId"
        class="pane"
      >
        <TerminalView :ref="setRef(tab.id)" :tab-id="tab.id" />
        <HexViewer v-show="hexOn[tab.id]" :ref="setHexRef(tab.id)" />
        <button
          class="hextoggle"
          :class="{ on: hexOn[tab.id] }"
          title="十六进制查看器"
          @click="toggleHex(tab.id)"
        >
          HEX
        </button>
        <!-- serial modem transfer bar -->
        <div v-if="tab.kind === 'serial'" class="modembar">
          <button @click="pickAndZsend(tab.id)">ZMODEM 发送</button>
          <button @click="sendModem(tab.id, 'XModem')">XMODEM 发送</button>
          <button @click="sendModem(tab.id, 'YModem')">YMODEM 发送</button>
          <button @click="recvModem(tab.id, 'XModem')">XMODEM 接收</button>
          <button @click="recvModem(tab.id, 'YModem')">YMODEM 接收</button>
          <span
            v-if="activeTab?.id === tab.id && activeModem && (activeModem.active || activeModem.text || activeModem.error)"
            class="mstatus"
            :class="{ err: !!activeModem.error }"
          >
            {{ activeModem.error || activeModem.text }}{{ activeModem.active ? modemPercent(activeModem) : "" }}
          </span>
        </div>
      </div>
    </div>
    <ComposePane v-if="composeOn" />
    <!-- trigger alert toasts -->
    <div class="toasts">
      <div v-for="t in toasts" :key="t.id" class="toast" @click="toasts = toasts.filter((x) => x.id !== t.id)">
        {{ t.text }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.tabs-wrap {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tabbar {
  display: flex;
  align-items: center;
  background: #252526;
  border-bottom: 1px solid #3c3c3c;
  min-height: 34px;
  overflow-x: auto;
}
.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 12px;
  height: 34px;
  border-right: 1px solid #333;
  color: #999;
  font-size: 12.5px;
  cursor: pointer;
  white-space: nowrap;
}
.tab.active {
  background: #1e1e1f;
  color: #eee;
  box-shadow: inset 0 2px 0 #0e639c;
}
.tab .x {
  visibility: hidden;
  color: #888;
  font-size: 11px;
}
.tab .detach {
  visibility: hidden;
  color: #888;
  font-size: 12px;
}
.tab:hover .x,
.tab.active .x,
.tab:hover .detach,
.tab.active .detach {
  visibility: visible;
}
.tab .x:hover {
  color: #fff;
}
.tab .detach:hover {
  color: #4fc3f7;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #666;
}
.dot.ok {
  background: #4ec9b0;
}
.dot.busy {
  background: #e2c08d;
}
.dot.err {
  background: #f14c4c;
}
.placeholder {
  color: #666;
  font-size: 12px;
  padding: 0 14px;
}
.tabbar-spacer {
  flex: 1;
}
.tabtool {
  background: #333;
  color: #999;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 10px;
  margin-right: 6px;
  cursor: pointer;
  white-space: nowrap;
}
.tabtool:hover {
  color: #eee;
}
.tabtool.on {
  background: #0e639c;
  border-color: #0e639c;
  color: #fff;
}
.tab .bc {
  color: #4ec9b0;
  font-size: 11px;
  cursor: pointer;
}
.tab .bc.off {
  color: #555;
  text-decoration: line-through;
}
.tab.bcout .title {
  opacity: 0.55;
}
.tab-body {
  flex: 1;
  min-height: 0;
  position: relative;
}
.pane {
  position: absolute;
  inset: 0;
}
.kind {
  color: #0e639c;
  text-transform: uppercase;
  font-size: 10px;
  border: 1px solid #0e639c;
  border-radius: 3px;
  padding: 0 3px;
  margin-right: 4px;
}
.hextoggle {
  position: absolute;
  top: 6px;
  right: 12px;
  z-index: 15;
  background: rgba(37, 37, 38, 0.85);
  color: #888;
  border: 1px solid #3c3c3c;
  border-radius: 4px;
  font-size: 10px;
  font-family: Consolas, monospace;
  padding: 2px 6px;
  cursor: pointer;
}
.hextoggle.on {
  color: #4fc3f7;
  border-color: #4fc3f7;
}
.modembar {
  position: absolute;
  right: 10px;
  bottom: 8px;
  display: flex;
  align-items: center;
  gap: 6px;
  background: rgba(37, 37, 38, 0.92);
  border: 1px solid #3c3c3c;
  border-radius: 6px;
  padding: 4px 8px;
  z-index: 10;
}
.modembar button {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 8px;
  cursor: pointer;
}
.modembar button:hover {
  background: #3c3c3c;
  color: #fff;
}
.mstatus {
  color: #9cdcfe;
  font-size: 11px;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mstatus.err {
  color: #f14c4c;
}
.toasts {
  position: fixed;
  right: 16px;
  bottom: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 400;
  pointer-events: none;
}
.toast {
  pointer-events: auto;
  background: rgba(40, 40, 42, 0.96);
  border: 1px solid #b58900;
  border-left: 3px solid #b58900;
  color: #e2c08d;
  border-radius: 6px;
  padding: 8px 14px;
  font-size: 12px;
  max-width: 360px;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5);
  cursor: pointer;
  animation: toast-in 0.18s ease-out;
}
@keyframes toast-in {
  from {
    transform: translateX(24px);
    opacity: 0;
  }
  to {
    transform: none;
    opacity: 1;
  }
}
</style>
