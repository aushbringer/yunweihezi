<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  b64decode,
  logDelete,
  logList,
  logOpenFolder,
  logTail,
  settingGet,
  settingSet,
  type LogListItem,
} from "../../ipc/client";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const logs = ref<LogListItem[]>([]);
const error = ref("");
const busy = ref(false);

// settings
const enabled = ref(false);
const dir = ref("");
const template = ref("%s_%d_%t.log");
const savedTip = ref("");

// preview
const previewPath = ref("");
const previewText = ref("");

async function refresh() {
  error.value = "";
  try {
    logs.value = await logList();
    logs.value.sort((a, b) => b.startedAt - a.startedAt);
  } catch (e) {
    error.value = String(e);
  }
}

async function loadSettings() {
  const [en, d, t] = await Promise.all([
    settingGet("log.enabled"),
    settingGet("log.dir"),
    settingGet("log.template"),
  ]);
  enabled.value = en === "true";
  dir.value = d ?? "";
  template.value = t || "%s_%d_%t.log";
}

async function saveSettings() {
  savedTip.value = "";
  try {
    await settingSet("log.enabled", enabled.value ? "true" : "false");
    await settingSet("log.dir", dir.value);
    await settingSet("log.template", template.value);
    savedTip.value = "已保存,对新会话生效";
    setTimeout(() => (savedTip.value = ""), 2500);
  } catch (e) {
    error.value = String(e);
  }
}

watch(visible, (v) => {
  if (v) {
    previewPath.value = "";
    previewText.value = "";
    void loadSettings();
    void refresh();
  }
});

function fmtTime(ms: number) {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function fmtSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function baseName(p: string) {
  return p.split(/[\\/]/).pop() ?? p;
}

async function preview(item: LogListItem) {
  error.value = "";
  busy.value = true;
  try {
    const b64 = await logTail(item.path, 64 * 1024);
    previewText.value = new TextDecoder().decode(b64decode(b64));
    previewPath.value = item.path;
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function remove(item: LogListItem, withFile: boolean) {
  error.value = "";
  try {
    await logDelete(item.id, withFile);
    if (previewPath.value === item.path) {
      previewPath.value = "";
      previewText.value = "";
    }
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

async function openFolder(item: LogListItem) {
  await logOpenFolder(item.path).catch((e) => (error.value = String(e)));
}
</script>

<template>
  <div v-if="visible" class="mask" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>日志审计</h3>
        <a class="x" @click="visible = false">✕</a>
      </div>

      <!-- settings -->
      <div class="sect">记录设置</div>
      <div class="grid">
        <label>启用</label>
        <div class="hrow">
          <label class="chk"><input v-model="enabled" type="checkbox" /> 会话输出自动落盘</label>
        </div>

        <label>目录</label>
        <input v-model="dir" placeholder="留空 = 应用数据目录/logs" />

        <label>命名模板</label>
        <input v-model="template" />
      </div>
      <div class="hint">
        变量: %h 主机 %u 用户 %s 会话名 %p 协议 %d 日期 %t 时间 %% 字面%
      </div>
      <div class="btns">
        <span v-if="savedTip" class="ok">{{ savedTip }}</span>
        <button class="primary" @click="saveSettings">保存设置</button>
      </div>

      <!-- log list -->
      <div class="sect">日志文件({{ logs.length }})</div>
      <div v-if="!logs.length" class="empty">暂无日志记录</div>
      <div v-for="item in logs" :key="item.id" class="rowitem">
        <div class="meta">
          <div class="name" :class="{ gone: !item.exists }" :title="item.path">
            {{ baseName(item.path) }}
          </div>
          <div class="sub">
            {{ fmtTime(item.startedAt) }} · {{ fmtSize(item.bytes) }}
            <span v-if="!item.exists" class="gone">(文件已删除)</span>
          </div>
        </div>
        <button class="mini" :disabled="!item.exists || busy" @click="preview(item)">预览</button>
        <button class="mini" :disabled="!item.exists" @click="openFolder(item)">目录</button>
        <button class="mini danger" @click="remove(item, false)">删记录</button>
        <button class="mini danger" :disabled="!item.exists" @click="remove(item, true)">删文件</button>
      </div>

      <!-- preview -->
      <template v-if="previewPath">
        <div class="sect">预览(尾部 64KB): {{ baseName(previewPath) }}</div>
        <pre class="preview">{{ previewText }}</pre>
      </template>

      <div v-if="error" class="err">{{ error }}</div>
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
  width: 640px;
  max-height: 84vh;
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
.grid {
  display: grid;
  grid-template-columns: 72px 1fr;
  gap: 8px 12px;
  align-items: center;
}
.grid label {
  color: #999;
  font-size: 12px;
  text-align: right;
}
input {
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
.hint {
  color: #666;
  font-size: 11px;
  margin-top: 6px;
}
.ok {
  color: #4ec9b0;
  font-size: 12px;
  margin-right: auto;
}
.rowitem {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 0;
}
.meta {
  flex: 1;
  min-width: 0;
  margin-right: 8px;
}
.name {
  font-family: Consolas, monospace;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.sub {
  color: #888;
  font-size: 11px;
}
.gone {
  color: #666;
}
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 8px;
  cursor: pointer;
  flex-shrink: 0;
}
.mini.danger {
  color: #f14c4c;
}
.mini:hover {
  background: #3c3c3c;
}
.mini:disabled {
  opacity: 0.4;
  cursor: default;
}
.preview {
  background: #1e1e1e;
  border: 1px solid #3c3c3c;
  border-radius: 4px;
  padding: 8px;
  font-family: Consolas, monospace;
  font-size: 11px;
  line-height: 1.45;
  max-height: 260px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
  margin: 0;
}
.err {
  color: #f14c4c;
  font-size: 12px;
  margin-top: 10px;
}
.btns {
  margin-top: 10px;
  display: flex;
  justify-content: flex-end;
  align-items: center;
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
