<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  ACTION_LABELS,
  useToolbarStore,
  type ToolbarAction,
  type ToolbarItem,
} from "../../stores/toolbar";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const tb = useToolbarStore();
const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

watch(visible, async (v) => {
  if (v) await tb.refreshQcmds();
});

const ACTIONS = Object.keys(ACTION_LABELS) as ToolbarAction[];

const addAction = ref<ToolbarAction>("ftp");
const addQcmd = ref("");

function label(item: ToolbarItem) {
  if (item.kind === "action") return ACTION_LABELS[item.action];
  return tb.qcmdById(item.id)?.label ?? "(快捷命令已删除)";
}

function kindLabel(item: ToolbarItem) {
  return item.kind === "action" ? "动作" : "快捷命令";
}

async function addSelected() {
  if (addQcmd.value) {
    await tb.add({ kind: "qcmd", id: addQcmd.value });
    addQcmd.value = "";
  } else {
    await tb.add({ kind: "action", action: addAction.value });
  }
}
</script>

<template>
  <div v-if="visible" class="overlay" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>自定义工具栏</h3>
        <span class="desc">调整顶栏按钮与顺序;FTP 等隐藏入口可在此添加</span>
        <button class="x" @click="visible = false">✕</button>
      </div>

      <table class="tbl">
        <thead>
          <tr><th class="c-idx">#</th><th>按钮</th><th class="c-kind">类型</th><th class="c-ops"></th></tr>
        </thead>
        <tbody>
          <tr v-for="(item, i) in tb.items" :key="i">
            <td class="c-idx">{{ i + 1 }}</td>
            <td>{{ label(item) }}</td>
            <td class="c-kind">{{ kindLabel(item) }}</td>
            <td class="c-ops">
              <a :class="{ dis: i === 0 }" @click="i > 0 && tb.move(i, -1)">↑</a>
              <a :class="{ dis: i === tb.items.length - 1 }" @click="i < tb.items.length - 1 && tb.move(i, 1)">↓</a>
              <a class="del" @click="tb.removeAt(i)">删除</a>
            </td>
          </tr>
          <tr v-if="!tb.items.length">
            <td colspan="4" class="empty">工具栏为空</td>
          </tr>
        </tbody>
      </table>

      <div class="addrow">
        <select v-model="addAction" :disabled="!!addQcmd">
          <option v-for="a in ACTIONS" :key="a" :value="a">{{ ACTION_LABELS[a] }}</option>
        </select>
        <select v-model="addQcmd">
          <option value="">(或选择快捷命令)</option>
          <option v-for="q in tb.qcmds" :key="q.id" :value="q.id">{{ q.label }}</option>
        </select>
        <button class="primary" @click="addSelected">添加</button>
        <span class="spacer" />
        <button class="mini" @click="tb.reset()">重置默认</button>
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
  width: 480px;
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
.c-idx {
  width: 30px;
  color: #666;
}
.c-kind {
  width: 70px;
  color: #888;
}
.c-ops {
  width: 90px;
  white-space: nowrap;
}
.c-ops a {
  color: #4fc3f7;
  cursor: pointer;
  margin-right: 8px;
  font-size: 12px;
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
.addrow {
  display: flex;
  gap: 8px;
  align-items: center;
}
.addrow .spacer {
  flex: 1;
}
select {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  color: #ddd;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 12.5px;
  outline: none;
  max-width: 180px;
}
.primary {
  background: #0e639c;
  border: 1px solid #0e639c;
  color: #fff;
  border-radius: 4px;
  padding: 5px 14px;
  font-size: 12px;
  cursor: pointer;
}
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 4px 10px;
  cursor: pointer;
}
</style>
