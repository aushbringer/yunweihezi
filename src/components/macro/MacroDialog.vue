<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useAppStore } from "../../stores/app";
import { useMacroStore, type Shortcut } from "../../stores/macros";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const store = useAppStore();
const ms = useMacroStore();

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const error = ref("");
const newName = ref("");

const activeSessionId = computed(
  () => store.tabs.find((t) => t.id === store.activeTabId)?.sessionId ?? ""
);

watch(visible, async (v) => {
  if (v) {
    error.value = "";
    if (!ms.loaded) await ms.load().catch((e) => (error.value = String(e)));
  } else if (ms.recording) {
    // closing the dialog while recording keeps recording off to avoid surprises
    await ms.stopRecord();
  }
});

async function toggleRecord() {
  if (ms.recording) {
    await ms.stopRecord();
    newName.value = "";
  } else {
    if (!newName.value.trim()) return;
    ms.startRecord(newName.value.trim());
  }
}

async function play(id: string) {
  if (!activeSessionId.value) return;
  const m = ms.macros.find((x) => x.id === id);
  if (!m) return;
  error.value = "";
  try {
    await ms.play(m, activeSessionId.value);
  } catch (e) {
    error.value = String(e);
  }
}

function remove(id: string) {
  const m = ms.macros.find((x) => x.id === id);
  if (m && !confirm(`删除宏「${m.name}」?`)) return;
  ms.remove(id);
}

// ---------- shortcut form ----------

const scForm = reactive({
  id: "",
  combo: "",
  text: "",
  macroId: "",
});

function captureCombo(e: KeyboardEvent) {
  e.preventDefault();
  const c = ms.comboOf(e);
  if (c) scForm.combo = c;
}

function saveShortcut() {
  if (!scForm.combo || (!scForm.text && !scForm.macroId)) return;
  const sc: Shortcut = {
    id: scForm.id || crypto.randomUUID(),
    combo: scForm.combo,
    text: scForm.macroId ? "" : scForm.text,
    macroId: scForm.macroId,
  };
  ms.upsertShortcut(sc);
  scForm.id = "";
  scForm.combo = "";
  scForm.text = "";
  scForm.macroId = "";
}

function editShortcut(sc: Shortcut) {
  scForm.id = sc.id;
  scForm.combo = sc.combo;
  scForm.text = sc.text;
  scForm.macroId = sc.macroId;
}

function shortcutDesc(sc: Shortcut) {
  if (sc.macroId) {
    const m = ms.macros.find((x) => x.id === sc.macroId);
    return `回放宏: ${m?.name ?? "(已删除)"}`;
  }
  return `发送: ${sc.text.length > 30 ? sc.text.slice(0, 30) + "…" : sc.text}`;
}
</script>

<template>
  <div v-if="visible" class="overlay" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>键盘宏 / 快捷键</h3>
        <span class="desc">录制终端键入并回放;快捷键对当前活动会话生效</span>
        <button class="x" @click="visible = false">✕</button>
      </div>

      <!-- macros -->
      <div class="sect">按键宏</div>
      <table class="tbl">
        <thead>
          <tr><th>名称</th><th class="c-num">步数</th><th class="c-ops"></th></tr>
        </thead>
        <tbody>
          <tr v-for="m in ms.macros" :key="m.id">
            <td>{{ m.name }}</td>
            <td class="c-num">{{ m.steps.length }}</td>
            <td class="c-ops">
              <a :class="{ dis: !activeSessionId }" @click="activeSessionId && play(m.id)">回放</a>
              <a class="del" @click="remove(m.id)">删除</a>
            </td>
          </tr>
          <tr v-if="!ms.macros.length">
            <td colspan="3" class="empty">暂无宏,在下方命名后开始录制</td>
          </tr>
        </tbody>
      </table>
      <div class="recrow">
        <input
          v-model="newName"
          placeholder="宏名称"
          :disabled="ms.recording"
          style="width: 180px"
        />
        <button class="primary" :class="{ rec: ms.recording }" :disabled="!ms.recording && !newName.trim()" @click="toggleRecord">
          {{ ms.recording ? `停止录制(已录 ${ms.recordSteps} 步)` : "开始录制" }}
        </button>
        <span v-if="ms.recording" class="hint">录制中:请切换到终端窗口键入,内容将按时间间隔记录</span>
      </div>

      <!-- shortcuts -->
      <div class="sect">快捷键映射(一键发送命令 / 回放宏)</div>
      <table class="tbl">
        <thead>
          <tr><th class="c-combo">快捷键</th><th>动作</th><th class="c-ops"></th></tr>
        </thead>
        <tbody>
          <tr v-for="sc in ms.shortcuts" :key="sc.id">
            <td class="c-combo mono">{{ sc.combo }}</td>
            <td>{{ shortcutDesc(sc) }}</td>
            <td class="c-ops">
              <a @click="editShortcut(sc)">编辑</a>
              <a class="del" @click="ms.removeShortcut(sc.id)">删除</a>
            </td>
          </tr>
          <tr v-if="!ms.shortcuts.length">
            <td colspan="3" class="empty">暂无快捷键</td>
          </tr>
        </tbody>
      </table>
      <div class="scform">
        <input
          :value="scForm.combo"
          placeholder="点击后按下组合键"
          readonly
          class="combo mono"
          @keydown="captureCombo"
        />
        <select v-model="scForm.macroId">
          <option value="">发送文本</option>
          <option v-for="m in ms.macros" :key="m.id" :value="m.id">回放宏: {{ m.name }}</option>
        </select>
        <input
          v-if="!scForm.macroId"
          v-model="scForm.text"
          placeholder="要发送的命令文本"
          style="flex: 1"
        />
        <button class="primary" :disabled="!scForm.combo || (!scForm.text && !scForm.macroId)" @click="saveShortcut">
          {{ scForm.id ? "保存" : "添加" }}
        </button>
      </div>

      <div v-if="error" class="err">{{ error }}</div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 300;
}
.panel {
  background: #252526;
  border: 1px solid #454545;
  border-radius: 8px;
  padding: 16px;
  width: 620px;
  max-width: 92vw;
  max-height: 84vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 10px;
}
.head h3 {
  margin: 0;
  color: #eee;
  font-size: 15px;
}
.head .desc {
  color: #777;
  font-size: 11.5px;
  flex: 1;
}
.head .x {
  background: none;
  border: none;
  color: #888;
  cursor: pointer;
  font-size: 13px;
}
.head .x:hover {
  color: #fff;
}
.sect {
  color: #888;
  font-size: 12px;
  border-bottom: 1px solid #333;
  padding-bottom: 4px;
  margin-top: 4px;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  color: #ccc;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 5px 8px;
  border-bottom: 1px solid #333;
}
.tbl th {
  color: #888;
  font-weight: 500;
}
.c-num {
  width: 60px;
}
.c-combo {
  width: 140px;
}
.c-ops {
  width: 90px;
  white-space: nowrap;
}
.c-ops a {
  color: #4fc3f7;
  cursor: pointer;
  margin-right: 10px;
  font-size: 11.5px;
}
.c-ops a.del {
  color: #f14c4c;
}
.c-ops a.dis {
  color: #555;
  cursor: default;
}
.empty {
  text-align: center;
  color: #555;
  padding: 12px 0;
}
.mono {
  font-family: Consolas, monospace;
}
input,
select {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 12.5px;
  outline: none;
}
.recrow,
.scform {
  display: flex;
  gap: 8px;
  align-items: center;
}
.scform .combo {
  width: 160px;
  cursor: pointer;
}
.primary {
  background: #0e639c;
  border: 1px solid #0e639c;
  color: #fff;
  border-radius: 4px;
  padding: 5px 14px;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.primary:disabled {
  opacity: 0.4;
  cursor: default;
}
.primary.rec {
  background: #8b3333;
  border-color: #8b3333;
}
.hint {
  color: #b58900;
  font-size: 11.5px;
}
.err {
  color: #f14c4c;
  font-size: 12px;
}
</style>
