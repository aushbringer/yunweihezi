<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useAppStore } from "../stores/app";
import { cryptoLock } from "../ipc/client";
import SessionTree from "../components/session/SessionTree.vue";
import SessionDialog from "../components/session/SessionDialog.vue";
import TerminalTabs from "../components/terminal/TerminalTabs.vue";
import SftpPanel from "../components/sftp/SftpPanel.vue";
import TransferQueue from "../components/sftp/TransferQueue.vue";
import TunnelPanel from "../components/forward/TunnelPanel.vue";
import LogViewer from "../components/logging/LogViewer.vue";
import QuickCommandBar from "../components/quickcmd/QuickCommandBar.vue";
import TriggerEditor from "../components/trigger/TriggerEditor.vue";
import MacroDialog from "../components/macro/MacroDialog.vue";
import ToolbarEditor from "../components/toolbar/ToolbarEditor.vue";
import FtpDialog from "../components/ftp/FtpDialog.vue";
import TerminalSettingsDialog from "../components/terminal/TerminalSettingsDialog.vue";
import UnlockDialog from "../components/session/UnlockDialog.vue";
import LockView from "../components/session/LockView.vue";
import { bindTransferWatch } from "../stores/transfers";
import { useTerminalSettings } from "../stores/terminalSettings";
import { ACTION_LABELS, useToolbarStore, type ToolbarAction } from "../stores/toolbar";
import { useMacroStore } from "../stores/macros";

const store = useAppStore();
const termSettings = useTerminalSettings();
const tb = useToolbarStore();
const ms = useMacroStore();

const showUnlock = ref(false);
const lockedScreen = ref(false);
const dialogVisible = ref(false);
const editSessionId = ref<string | null>(null);
const showTunnels = ref(false);
const showLogs = ref(false);
const showQuickCmd = ref(false);
const showTriggers = ref(false);
const showMacros = ref(false);
const showToolbarEditor = ref(false);
const showFtp = ref(false);
const showTermSettings = ref(false);

const activeTab = computed(() => store.tabs.find((t) => t.id === store.activeTabId) ?? null);
const sftpReady = computed(
  () => activeTab.value?.sftpVisible && activeTab.value?.sftpId && activeTab.value?.sessionId
);

/** Apply the whole-window opacity setting to the #app element. */
function applyOpacity(v: number) {
  const el = document.getElementById("app");
  if (el) el.style.opacity = String(v);
}

watch(() => termSettings.opacity, applyOpacity);

onMounted(async () => {
  bindTransferWatch();
  await termSettings.load().catch(() => {});
  applyOpacity(termSettings.opacity);
  void tb.load().catch(() => {});
  void ms.load().catch(() => {});
  window.addEventListener("keydown", onGlobalKey, true);
  await store.refreshCrypto();
  await store.refreshAll();
  if (!store.unlocked) showUnlock.value = true;
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKey, true);
});

/** Global custom shortcuts (发送命令 / 回放宏 to the active session). */
function onGlobalKey(e: KeyboardEvent) {
  const combo = ms.comboOf(e);
  if (!combo || !ms.shortcuts.some((s) => s.combo === combo)) return;
  const sid = activeTab.value?.sessionId;
  if (!sid) return;
  e.preventDefault();
  void ms.fireShortcut(combo, sid).catch(() => {});
}

function actionDisabled(a: ToolbarAction) {
  switch (a) {
    case "sftp":
      return !activeTab.value?.sessionId;
    case "tunnels":
      return activeTab.value?.kind !== "ssh" || !activeTab.value?.sessionId;
    default:
      return false;
  }
}

function actionTitle(a: ToolbarAction) {
  switch (a) {
    case "sftp":
      return activeTab.value?.sftpVisible ? "关闭 SFTP 面板" : "打开 SFTP 面板";
    case "tunnels":
      return "端口转发(L/R/D)";
    case "logs":
      return "会话日志审计";
    case "quickcmd":
      return "显示/隐藏快捷命令栏";
    case "triggers":
      return "触发器自动化规则";
    case "macros":
      return "键盘宏 / 快捷键映射";
    case "termSettings":
      return "字体/主题/搜索高亮";
    case "ftp":
      return "明文 FTP 文件传输";
    default:
      return "";
  }
}

function runAction(a: ToolbarAction) {
  switch (a) {
    case "newSession":
      newSession();
      break;
    case "sftp":
      void toggleSftp();
      break;
    case "tunnels":
      showTunnels.value = true;
      break;
    case "logs":
      showLogs.value = true;
      break;
    case "quickcmd":
      showQuickCmd.value = !showQuickCmd.value;
      break;
    case "triggers":
      showTriggers.value = true;
      break;
    case "macros":
      showMacros.value = true;
      break;
    case "termSettings":
      showTermSettings.value = true;
      break;
    case "ftp":
      showFtp.value = true;
      break;
    case "lock":
      void lockNow();
      break;
  }
}

function newSession() {
  editSessionId.value = null;
  dialogVisible.value = true;
}

function editSession(id: string) {
  editSessionId.value = id;
  dialogVisible.value = true;
}

async function onUnlockDone() {
  showUnlock.value = false;
  await store.refreshCrypto();
  await store.refreshAll();
}

async function lockNow() {
  await cryptoLock();
  await store.refreshCrypto();
  // Full lock screen only when a master password exists; otherwise the
  // first-run dialog (which offers master setup) is the right flow.
  if (store.masterSet) {
    lockedScreen.value = true;
  } else {
    showUnlock.value = true;
  }
}

async function onUnlockFromLock() {
  lockedScreen.value = false;
  await store.refreshCrypto();
}

async function toggleSftp() {
  const tab = activeTab.value;
  if (!tab || !tab.sessionId) return;
  if (tab.sftpVisible) {
    tab.sftpVisible = false;
  } else {
    try {
      await store.ensureSftp(tab.id);
    } catch (e) {
      alert(`SFTP 打开失败: ${e}`);
    }
  }
}
</script>

<template>
  <div class="main-view">
    <header class="topbar">
      <span class="brand">OpenTerm</span>
      <template v-for="(item, i) in tb.items" :key="i">
        <button
          v-if="item.kind === 'action'"
          class="tb-btn"
          :class="{ on: item.action === 'quickcmd' && showQuickCmd }"
          :disabled="actionDisabled(item.action)"
          :title="actionTitle(item.action)"
          @click="runAction(item.action)"
        >
          {{ ACTION_LABELS[item.action] }}
        </button>
        <button
          v-else
          class="tb-btn"
          :disabled="!activeTab?.sessionId"
          :title="tb.qcmdById(item.id)?.content ?? ''"
          @click="tb.runQcmd(item.id)"
        >
          {{ tb.qcmdById(item.id)?.label ?? "?" }}
        </button>
      </template>
      <button class="tb-btn layout" title="自定义工具栏" @click="showToolbarEditor = true">布局</button>
      <span class="spacer" />
      <span class="vault-state" :class="{ locked: !store.unlocked }">
        {{ store.unlocked ? "已解锁" : "已锁定" }}
      </span>
    </header>

    <div class="body">
      <aside class="sidebar">
        <SessionTree @new-session="newSession" @edit-session="editSession" />
      </aside>
      <section class="work">
        <QuickCommandBar v-if="showQuickCmd" />
        <TerminalTabs />
        <div v-if="sftpReady" class="sftp-drawer">
          <div class="panes">
            <SftpPanel kind="local" />
            <div class="pane-sep" />
            <SftpPanel kind="remote" :sftp-id="activeTab!.sftpId!" :session-id="activeTab!.sessionId" />
          </div>
          <TransferQueue />
        </div>
      </section>
    </div>

    <SessionDialog v-model="dialogVisible" :edit-id="editSessionId" />
    <TunnelPanel v-model="showTunnels" />
    <LogViewer v-model="showLogs" />
    <TriggerEditor v-model="showTriggers" />
    <MacroDialog v-model="showMacros" />
    <ToolbarEditor v-model="showToolbarEditor" />
    <FtpDialog v-model="showFtp" />
    <TerminalSettingsDialog v-model="showTermSettings" />
    <UnlockDialog v-if="showUnlock" @done="onUnlockDone" />
    <LockView v-if="lockedScreen" @done="onUnlockFromLock" />
  </div>
</template>

<style scoped>
.main-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.topbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: #252526;
  border-bottom: 1px solid #3c3c3c;
}
.brand {
  font-weight: 700;
  color: #4fc3f7;
  margin-right: 6px;
}
.tb-btn {
  background: #37373d;
  color: #ddd;
  border: 1px solid #4a4a4a;
  padding: 4px 12px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}
.tb-btn:hover:not(:disabled) {
  background: #454549;
}
.tb-btn.on {
  background: #0e639c;
  border-color: #0e639c;
  color: #fff;
}
.tb-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.spacer {
  flex: 1;
}
.vault-state {
  font-size: 12px;
  color: #7ee787;
}
.vault-state.locked {
  color: #f7a54f;
}
.body {
  flex: 1;
  display: flex;
  min-height: 0;
}
.sidebar {
  width: 240px;
  min-width: 240px;
  background: #212122;
  border-right: 1px solid #3c3c3c;
  overflow: hidden;
  display: flex;
}
.work {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.sftp-drawer {
  height: 320px;
  min-height: 200px;
  border-top: 1px solid #3c3c3c;
  display: flex;
  flex-direction: column;
  background: #1b1b1c;
}
.panes {
  flex: 1;
  display: flex;
  min-height: 0;
}
.pane-sep {
  width: 1px;
  background: #3c3c3c;
}
</style>
