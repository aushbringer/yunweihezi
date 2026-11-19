<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useAppStore } from "../../stores/app";
import {
  triggerDelete,
  triggerList,
  triggerUpsert,
  type TriggerRule,
} from "../../ipc/client";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const store = useAppStore();
const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const rules = ref<TriggerRule[]>([]);
const error = ref("");

async function refresh() {
  error.value = "";
  try {
    rules.value = await triggerList();
  } catch (e) {
    error.value = String(e);
  }
}

watch(visible, (v) => {
  if (v) {
    void refresh();
    resetForm();
  }
});

// ---------- edit form ----------

const ACTIONS = [
  { value: "highlight", label: "高亮" },
  { value: "alert", label: "告警" },
  { value: "send", label: "发送命令" },
  { value: "script", label: "执行脚本" },
];

const form = reactive({
  id: "" as string | undefined,
  name: "",
  pattern: "",
  isRegex: false,
  action: "highlight",
  actionParam: "",
  sessionId: "" as string, // "" = 全局
  enabled: true,
});

function resetForm() {
  form.id = undefined;
  form.name = "";
  form.pattern = "";
  form.isRegex = false;
  form.action = "highlight";
  form.actionParam = "";
  form.sessionId = "";
  form.enabled = true;
}

function edit(r: TriggerRule) {
  form.id = r.id;
  form.name = r.name;
  form.pattern = r.pattern;
  form.isRegex = r.isRegex;
  form.action = r.action;
  form.actionParam = r.actionParam;
  form.sessionId = r.sessionId ?? "";
  form.enabled = r.enabled;
}

async function save() {
  if (!form.name.trim() || !form.pattern) return;
  error.value = "";
  try {
    await triggerUpsert({
      id: form.id,
      sessionId: form.sessionId || null,
      name: form.name.trim(),
      pattern: form.pattern,
      isRegex: form.isRegex,
      action: form.action,
      actionParam: form.actionParam,
      enabled: form.enabled,
    });
    resetForm();
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

async function remove(r: TriggerRule) {
  if (!confirm(`删除触发器「${r.name}」?`)) return;
  await triggerDelete(r.id).catch((e) => (error.value = String(e)));
  if (form.id === r.id) resetForm();
  await refresh();
}

async function toggleEnabled(r: TriggerRule) {
  try {
    await triggerUpsert({
      id: r.id,
      sessionId: r.sessionId,
      name: r.name,
      pattern: r.pattern,
      isRegex: r.isRegex,
      action: r.action,
      actionParam: r.actionParam,
      enabled: !r.enabled,
    });
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

function actionLabel(a: string) {
  return ACTIONS.find((x) => x.value === a)?.label ?? a;
}

function sessionName(id: string | null) {
  if (!id) return "全局";
  return store.sessions.find((s) => s.id === id)?.name ?? id.slice(0, 8);
}

const paramHint = computed(() => {
  switch (form.action) {
    case "highlight":
      return "颜色,如 #b58900(可空)";
    case "alert":
      return "提示文本,支持 {match} 变量(可空)";
    case "send":
      return "匹配后发送的命令,支持 {match} 变量";
    case "script":
      return "Python 脚本内容或 @脚本路径(需 python feature 编译)";
    default:
      return "";
  }
});
</script>

<template>
  <div v-if="visible" class="overlay" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>触发器</h3>
        <span class="desc">匹配会话输出后自动执行动作;绑定会话为空时对全部会话生效</span>
        <button class="x" @click="visible = false">✕</button>
      </div>

      <table class="rules">
        <thead>
          <tr>
            <th class="c-enable">启用</th>
            <th>名称</th>
            <th>模式</th>
            <th class="c-regex">正则</th>
            <th>动作</th>
            <th>绑定会话</th>
            <th class="c-ops"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in rules" :key="r.id" :class="{ off: !r.enabled }">
            <td class="c-enable">
              <input type="checkbox" :checked="r.enabled" @change="toggleEnabled(r)" />
            </td>
            <td>{{ r.name }}</td>
            <td class="mono" :title="r.pattern">{{ r.pattern }}</td>
            <td class="c-regex">{{ r.isRegex ? "是" : "" }}</td>
            <td>{{ actionLabel(r.action) }}</td>
            <td>{{ sessionName(r.sessionId) }}</td>
            <td class="c-ops">
              <a @click="edit(r)">编辑</a>
              <a class="del" @click="remove(r)">删除</a>
            </td>
          </tr>
          <tr v-if="!rules.length">
            <td colspan="7" class="empty">暂无规则,在下方新建</td>
          </tr>
        </tbody>
      </table>

      <div class="form">
        <div class="frow">
          <label class="w-name">
            名称
            <input v-model="form.name" placeholder="规则名称" />
          </label>
          <label class="w-pattern">
            匹配模式
            <input v-model="form.pattern" placeholder="文本或正则表达式" />
          </label>
          <label class="w-check">
            <input v-model="form.isRegex" type="checkbox" />
            正则
          </label>
        </div>
        <div class="frow">
          <label class="w-action">
            动作
            <select v-model="form.action">
              <option v-for="a in ACTIONS" :key="a.value" :value="a.value">{{ a.label }}</option>
            </select>
          </label>
          <label class="w-param">
            参数
            <input v-model="form.actionParam" :placeholder="paramHint" />
          </label>
          <label class="w-session">
            绑定会话
            <select v-model="form.sessionId">
              <option value="">全局</option>
              <option v-for="s in store.sessions" :key="s.id" :value="s.id">{{ s.name }}</option>
            </select>
          </label>
          <label class="w-check">
            <input v-model="form.enabled" type="checkbox" />
            启用
          </label>
        </div>
        <div class="frow btns">
          <span v-if="error" class="err">{{ error }}</span>
          <span class="spacer" />
          <button v-if="form.id" @click="resetForm">取消编辑</button>
          <button class="primary" :disabled="!form.name.trim() || !form.pattern" @click="save">
            {{ form.id ? "保存修改" : "新增规则" }}
          </button>
        </div>
      </div>
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
  width: 760px;
  max-width: 92vw;
  max-height: 84vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
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
.rules {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  color: #ccc;
}
.rules th,
.rules td {
  text-align: left;
  padding: 5px 8px;
  border-bottom: 1px solid #333;
}
.rules th {
  color: #888;
  font-weight: 500;
}
.rules tr.off td {
  opacity: 0.45;
}
.rules .mono {
  font-family: Consolas, monospace;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.c-enable,
.c-regex {
  width: 44px;
  text-align: center;
}
.c-ops {
  width: 80px;
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
.empty {
  text-align: center;
  color: #555;
  padding: 14px 0;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  border-top: 1px solid #333;
  padding-top: 12px;
}
.frow {
  display: flex;
  gap: 10px;
  align-items: flex-end;
}
.form label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  color: #aaa;
  font-size: 12px;
}
.form input[type="text"],
.form input:not([type]),
.form select {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 12.5px;
  outline: none;
}
.w-name {
  width: 160px;
}
.w-pattern {
  flex: 1;
}
.w-action {
  width: 110px;
}
.w-param {
  flex: 1;
}
.w-session {
  width: 160px;
}
.w-check {
  flex-direction: row;
  align-items: center;
  gap: 4px;
  padding-bottom: 6px;
  white-space: nowrap;
}
.btns {
  align-items: center;
}
.btns .spacer {
  flex: 1;
}
.btns .err {
  color: #f14c4c;
  font-size: 12px;
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
</style>
