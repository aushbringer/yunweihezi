<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useAppStore } from "../../stores/app";
import {
  tunnelRunning,
  tunnelSavedDelete,
  tunnelSavedList,
  tunnelSavedUpsert,
  tunnelStart,
  tunnelStop,
  type RunningTunnel,
  type TunnelRecord,
} from "../../ipc/client";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const store = useAppStore();
const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const activeSshTab = computed(() =>
  store.tabs.find((t) => t.id === store.activeTabId && t.kind === "ssh" && t.sessionId)
);

const running = ref<RunningTunnel[]>([]);
const saved = ref<TunnelRecord[]>([]);
const error = ref("");
const busy = ref(false);

const form = reactive({
  kind: "local" as "local" | "remote" | "dynamic",
  listenHost: "127.0.0.1",
  listenPort: 8080,
  targetHost: "127.0.0.1",
  targetPort: 3306,
  name: "",
  autostart: false,
  saveToo: false,
});

async function refresh() {
  error.value = "";
  try {
    const sid = activeSshTab.value?.sessionId;
    running.value = sid ? await tunnelRunning(sid) : [];
    const recordId = activeSshTab.value?.recordId;
    saved.value = recordId ? await tunnelSavedList(recordId) : [];
  } catch (e) {
    error.value = String(e);
  }
}

watch(visible, (v) => {
  if (v) void refresh();
});

function kindLabel(k: string) {
  return k === "local" ? "本地" : k === "remote" ? "远程" : "动态";
}

async function start() {
  const tab = activeSshTab.value;
  if (!tab) return;
  error.value = "";
  busy.value = true;
  try {
    await tunnelStart({
      sessionId: tab.sessionId,
      kind: form.kind,
      bindHost: form.listenHost,
      bindPort: form.listenPort,
      targetHost: form.targetHost,
      targetPort: form.targetPort,
    });
    if (form.saveToo && tab.recordId) {
      await tunnelSavedUpsert({
        id: "",
        sessionId: tab.recordId,
        name: form.name || `${form.kind}:${form.listenPort}`,
        kind: form.kind,
        listenHost: form.listenHost,
        listenPort: form.listenPort,
        targetHost: form.targetHost,
        targetPort: form.targetPort,
        autostart: form.autostart,
      });
    }
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function startSaved(rec: TunnelRecord) {
  const tab = activeSshTab.value;
  if (!tab) return;
  error.value = "";
  try {
    await tunnelStart({
      sessionId: tab.sessionId,
      kind: rec.kind,
      bindHost: rec.listenHost,
      bindPort: rec.listenPort,
      targetHost: rec.targetHost,
      targetPort: rec.targetPort,
    });
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

async function stop(id: string) {
  await tunnelStop(id).catch((e) => (error.value = String(e)));
  await refresh();
}

async function removeSaved(id: string) {
  await tunnelSavedDelete(id).catch((e) => (error.value = String(e)));
  await refresh();
}
</script>

<template>
  <div v-if="visible" class="mask" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>端口转发</h3>
        <a class="x" @click="visible = false">✕</a>
      </div>

      <div v-if="!activeSshTab" class="empty">当前活动标签页不是已连接的 SSH 会话</div>
      <template v-else>
        <!-- running -->
        <div class="sect">运行中({{ running.length }})</div>
        <div v-if="!running.length" class="empty">无</div>
        <div v-for="t in running" :key="t.id" class="rowitem">
          <span class="badge" :class="t.kind">{{ kindLabel(t.kind) }}</span>
          <span class="addr">{{ t.bindHost }}:{{ t.bindPort }}</span>
          <span class="arrow">→</span>
          <span class="addr">{{ t.kind === "dynamic" ? "(SOCKS5)" : `${t.targetHost}:${t.targetPort}` }}</span>
          <button class="mini danger" @click="stop(t.id)">停止</button>
        </div>

        <!-- saved -->
        <div class="sect">已保存({{ saved.length }})</div>
        <div v-if="!saved.length" class="empty">无</div>
        <div v-for="t in saved" :key="t.id" class="rowitem">
          <span class="badge" :class="t.kind">{{ kindLabel(t.kind) }}</span>
          <span class="addr">{{ t.listenHost }}:{{ t.listenPort }}</span>
          <span class="arrow">→</span>
          <span class="addr">{{ t.kind === "dynamic" ? "(SOCKS5)" : `${t.targetHost}:${t.targetPort}` }}</span>
          <span v-if="t.autostart" class="auto">自启</span>
          <button class="mini" @click="startSaved(t)">启动</button>
          <button class="mini danger" @click="removeSaved(t.id)">删除</button>
        </div>

        <!-- new -->
        <div class="sect">新建隧道</div>
        <div class="grid">
          <label>类型</label>
          <select v-model="form.kind">
            <option value="local">本地转发(-L)</option>
            <option value="remote">远程转发(-R)</option>
            <option value="dynamic">动态 SOCKS5(-D)</option>
          </select>

          <label>监听</label>
          <div class="hrow">
            <input v-model="form.listenHost" style="flex: 1" />
            <input v-model.number="form.listenPort" type="number" style="width: 90px" />
          </div>

          <template v-if="form.kind !== 'dynamic'">
            <label>目标</label>
            <div class="hrow">
              <input v-model="form.targetHost" style="flex: 1" />
              <input v-model.number="form.targetPort" type="number" style="width: 90px" />
            </div>
          </template>

          <label>选项</label>
          <div class="hrow checks">
            <label class="chk"><input v-model="form.saveToo" type="checkbox" /> 保存定义</label>
            <label class="chk" v-if="form.saveToo"><input v-model="form.autostart" type="checkbox" /> 随会话自启</label>
          </div>
        </div>

        <div v-if="error" class="err">{{ error }}</div>
        <div class="btns">
          <button class="primary" :disabled="busy" @click="start">
            {{ busy ? "启动中…" : "启动隧道" }}
          </button>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.panel {
  width: 520px;
  max-height: 80vh;
  overflow-y: auto;
  background: #252526;
  border: 1px solid #3c3c3c;
  border-radius: 8px;
  padding: 16px 18px;
  color: #ddd;
  font-size: 13px;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}
.head h3 {
  margin: 0;
  font-size: 15px;
}
.x {
  cursor: pointer;
  color: #999;
}
.sect {
  color: #888;
  font-size: 12px;
  margin: 14px 0 6px;
  border-bottom: 1px solid #333;
  padding-bottom: 4px;
}
.empty {
  color: #666;
  font-size: 12px;
  padding: 4px 0;
}
.rowitem {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 0;
}
.badge {
  font-size: 11px;
  border-radius: 3px;
  padding: 1px 6px;
  border: 1px solid #555;
  color: #aaa;
  flex-shrink: 0;
}
.badge.local {
  color: #4ec9b0;
  border-color: #4ec9b0;
}
.badge.remote {
  color: #dcdcaa;
  border-color: #dcdcaa;
}
.badge.dynamic {
  color: #c586c0;
  border-color: #c586c0;
}
.addr {
  font-family: Consolas, monospace;
  font-size: 12px;
}
.arrow {
  color: #666;
}
.auto {
  font-size: 11px;
  color: #4ec9b0;
}
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 10px;
  cursor: pointer;
  margin-left: auto;
}
.mini.danger {
  margin-left: 0;
  color: #f14c4c;
}
.mini + .mini {
  margin-left: 6px;
}
.mini:hover {
  background: #3c3c3c;
}
.grid {
  display: grid;
  grid-template-columns: 56px 1fr;
  gap: 8px 12px;
  align-items: center;
}
.grid label {
  color: #999;
  font-size: 12px;
  text-align: right;
}
input,
select {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 13px;
  outline: none;
  box-sizing: border-box;
  width: 100%;
}
.hrow {
  display: flex;
  gap: 8px;
}
.checks {
  align-items: center;
}
.chk {
  display: flex;
  align-items: center;
  gap: 5px;
  color: #aaa;
  font-size: 12px;
}
.chk input {
  width: auto;
}
.err {
  color: #f14c4c;
  font-size: 12px;
  margin-top: 10px;
}
.btns {
  margin-top: 14px;
  display: flex;
  justify-content: flex-end;
}
.primary {
  background: #0e639c;
  color: #fff;
  border: none;
  border-radius: 4px;
  padding: 6px 16px;
  cursor: pointer;
}
</style>
