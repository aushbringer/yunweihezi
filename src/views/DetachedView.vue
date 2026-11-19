<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import TerminalView from "../components/terminal/TerminalView.vue";
import { sessionAttach, sessionClose, type OutputEvent } from "../ipc/client";
import { useTerminalSettings } from "../stores/terminalSettings";

/**
 * Standalone terminal window: takes over a running backend session that was
 * detached from the main window (see TerminalTabs.detachTab). The previous
 * buffer content is handed over through localStorage and replayed via the
 * xterm serialize buffer.
 */
const termRef = ref<InstanceType<typeof TerminalView> | null>(null);
const title = ref("终端");
const state = ref("connecting");
let sessionId = "";

function handleOutput(ev: OutputEvent) {
  if (ev.kind === "state") {
    state.value = ev.state;
    // State events are also shown inline by TerminalView via emit; forward.
    termRef.value?.handleOutput(ev);
    return;
  }
  termRef.value?.handleOutput(ev);
}

onMounted(async () => {
  const ts = useTerminalSettings();
  await ts.load().catch(() => {});

  const params = new URLSearchParams(location.search);
  sessionId = params.get("detach") ?? "";
  let serialized = "";
  const raw = localStorage.getItem(`detach:${sessionId}`);
  if (raw) {
    try {
      const payload = JSON.parse(raw) as { title?: string; serialized?: string };
      title.value = payload.title ?? title.value;
      serialized = payload.serialized ?? "";
    } catch {
      /* ignore corrupt payload */
    }
    localStorage.removeItem(`detach:${sessionId}`);
  }

  const win = getCurrentWindow();
  await win.setTitle(`${title.value} — OpenTerm`).catch(() => {});

  // Re-point the backend session's output stream at this window.
  try {
    await sessionAttach(sessionId, handleOutput);
    termRef.value?.attach(sessionId);
    if (serialized) termRef.value?.writeRaw(serialized);
    termRef.value?.focus();
  } catch (e) {
    termRef.value?.writeRaw(`\r\n\x1b[31m[attach failed: ${e}]\x1b[0m\r\n`);
    state.value = "error";
  }

  win.onCloseRequested(async (event) => {
    event.preventDefault();
    if (state.value === "connected" || state.value === "connecting") {
      if (!confirm("会话仍在连接中,确定关闭?")) return;
    }
    if (sessionId) await sessionClose(sessionId).catch(() => {});
    await win.destroy();
  });
});
</script>

<template>
  <div class="detached">
    <div class="dhead">
      <span class="dot" :class="state" />
      <span class="t">{{ title }}</span>
      <span class="sid">{{ sessionId.slice(0, 8) }}</span>
    </div>
    <div class="dbody">
      <TerminalView ref="termRef" />
    </div>
  </div>
</template>

<style scoped>
.detached {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.dhead {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  background: #252526;
  border-bottom: 1px solid #3c3c3c;
  font-size: 12px;
  color: #bbb;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #666;
}
.dot.connected {
  background: #4ec9b0;
}
.dot.connecting {
  background: #e2c08d;
}
.dot.error,
.dot.disconnected {
  background: #f14c4c;
}
.t {
  font-weight: 600;
  color: #eee;
}
.sid {
  margin-left: auto;
  color: #666;
  font-family: Consolas, monospace;
  font-size: 11px;
}
.dbody {
  flex: 1;
  min-height: 0;
}
</style>
