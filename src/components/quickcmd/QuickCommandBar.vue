<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useAppStore } from "../../stores/app";
import {
  quickCommandDelete,
  quickCommandList,
  quickCommandUpsert,
  sessionInputText,
  type QuickCommand,
} from "../../ipc/client";

const store = useAppStore();
const commands = ref<QuickCommand[]>([]);
const error = ref("");

const activeSessionId = computed(
  () => store.tabs.find((t) => t.id === store.activeTabId)?.sessionId ?? ""
);

async function refresh() {
  try {
    commands.value = await quickCommandList();
  } catch (e) {
    error.value = String(e);
  }
}

onMounted(refresh);

/** Send command content into the active session; newlines become Enter. */
async function run(cmd: QuickCommand) {
  const sid = activeSessionId.value;
  if (!sid) return;
  let text = cmd.content.replace(/\r?\n/g, "\r");
  if (!text.endsWith("\r")) text += "\r";
  await sessionInputText(sid, text).catch((e) => (error.value = String(e)));
}

// ---------- edit dialog ----------

const editing = ref(false);
const form = reactive({
  id: "" as string | undefined,
  label: "",
  content: "",
  group: "",
  sort: 0,
});

function openNew() {
  form.id = undefined;
  form.label = "";
  form.content = "";
  form.group = "";
  form.sort = commands.value.length;
  editing.value = true;
}

function openEdit(cmd: QuickCommand) {
  form.id = cmd.id;
  form.label = cmd.label;
  form.content = cmd.content;
  form.group = cmd.group;
  form.sort = cmd.sort;
  editing.value = true;
}

async function save() {
  if (!form.label.trim()) return;
  try {
    await quickCommandUpsert({
      id: form.id,
      label: form.label.trim(),
      content: form.content,
      group: form.group.trim(),
      sort: form.sort,
    });
    editing.value = false;
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

async function remove() {
  if (!form.id) return;
  if (!confirm(`删除快捷命令「${form.label}」?`)) return;
  await quickCommandDelete(form.id).catch((e) => (error.value = String(e)));
  editing.value = false;
  await refresh();
}
</script>

<template>
  <div class="qcmd-bar">
    <div class="qcmd-list">
      <button
        v-for="cmd in commands"
        :key="cmd.id"
        class="qcmd"
        :title="cmd.content"
        :disabled="!activeSessionId"
        @click="run(cmd)"
        @contextmenu.prevent="openEdit(cmd)"
      >
        {{ cmd.label }}
      </button>
      <span v-if="!commands.length" class="hint">右键按钮可编辑,点击「+」新增快捷命令</span>
    </div>
    <button class="qcmd add" title="新增快捷命令" @click="openNew">+</button>

    <div v-if="editing" class="overlay" @click.self="editing = false">
      <div class="dialog">
        <h3>{{ form.id ? "编辑快捷命令" : "新增快捷命令" }}</h3>
        <label>
          名称
          <input v-model="form.label" placeholder="按钮显示文字" autofocus />
        </label>
        <label>
          内容(多行,每行一条命令)
          <textarea v-model="form.content" rows="5" placeholder="例如: df -h"></textarea>
        </label>
        <div class="row">
          <label>
            分组
            <input v-model="form.group" placeholder="可空" />
          </label>
          <label class="sort">
            排序
            <input v-model.number="form.sort" type="number" />
          </label>
        </div>
        <div class="btns">
          <button v-if="form.id" class="danger" @click="remove">删除</button>
          <span class="spacer" />
          <button @click="editing = false">取消</button>
          <button class="primary" :disabled="!form.label.trim()" @click="save">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.qcmd-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 8px;
  background: #222223;
  border-bottom: 1px solid #3c3c3c;
}
.qcmd-list {
  display: flex;
  align-items: center;
  gap: 4px;
  overflow-x: auto;
  flex: 1;
  min-width: 0;
}
.qcmd {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11.5px;
  padding: 2px 10px;
  cursor: pointer;
  white-space: nowrap;
}
.qcmd:hover:not(:disabled) {
  background: #3c3c3c;
  color: #fff;
}
.qcmd:disabled {
  opacity: 0.4;
  cursor: default;
}
.qcmd.add {
  color: #4fc3f7;
  border-color: #0e639c;
  font-weight: 700;
}
.hint {
  color: #555;
  font-size: 11px;
  white-space: nowrap;
}
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 300;
}
.dialog {
  background: #252526;
  border: 1px solid #454545;
  border-radius: 8px;
  padding: 16px;
  width: 420px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.dialog h3 {
  margin: 0;
  color: #eee;
  font-size: 14px;
}
.dialog label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  color: #aaa;
  font-size: 12px;
  flex: 1;
}
.dialog input,
.dialog textarea {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 12.5px;
  outline: none;
  font-family: inherit;
}
.dialog textarea {
  font-family: Consolas, monospace;
  resize: vertical;
}
.row {
  display: flex;
  gap: 10px;
}
.row .sort {
  max-width: 90px;
}
.btns {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
.btns .spacer {
  flex: 1;
}
.btns button {
  background: #37373d;
  color: #ddd;
  border: 1px solid #4a4a4a;
  padding: 5px 14px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}
.btns button.primary {
  background: #0e639c;
  border-color: #0e639c;
  color: #fff;
}
.btns button.primary:disabled {
  opacity: 0.4;
  cursor: default;
}
.btns button.danger {
  background: #5a1d1d;
  border-color: #8b3333;
  color: #f0a0a0;
}
</style>
