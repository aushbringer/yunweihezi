<script setup lang="ts">
import { computed, watch } from "vue";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTerminalSettings } from "../../stores/terminalSettings";
import { BUILTIN_THEMES } from "./themes";
import { textFileRead, textFileWrite } from "../../ipc/files";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const ts = useTerminalSettings();

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const error = defineModel<string>("error", { default: "" });
const tip = defineModel<string>("tip", { default: "" });

watch(visible, (v) => {
  if (v) {
    error.value = "";
    tip.value = "";
  }
});

async function apply() {
  error.value = "";
  try {
    await ts.save();
    tip.value = "已保存并应用";
    setTimeout(() => (tip.value = ""), 2000);
  } catch (e) {
    error.value = String(e);
  }
}

async function importTheme() {
  error.value = "";
  const picked = await openDialog({
    multiple: false,
    directory: false,
    filters: [{ name: "主题 JSON", extensions: ["json"] }],
  });
  if (typeof picked !== "string") return;
  try {
    const content = await textFileRead(picked);
    const parsed = JSON.parse(content);
    // Accept either a raw ITheme or { name, theme }.
    const theme = parsed.theme ?? parsed;
    if (typeof theme !== "object" || (!theme.background && !theme.foreground)) {
      throw new Error("不是有效的终端主题 JSON(至少需要 background/foreground)");
    }
    ts.setCustomTheme(theme);
    tip.value = "主题已导入(点击保存生效)";
  } catch (e) {
    error.value = String(e);
  }
}

async function exportTheme() {
  error.value = "";
  const theme = ts.theme();
  if (!theme) return;
  const path = await saveDialog({
    defaultPath: "openterm-theme.json",
    filters: [{ name: "主题 JSON", extensions: ["json"] }],
  });
  if (!path) return;
  try {
    await textFileWrite(
      path,
      JSON.stringify({ name: ts.customTheme ? "custom" : ts.themeName, theme }, null, 2)
    );
    tip.value = "已导出";
    setTimeout(() => (tip.value = ""), 2000);
  } catch (e) {
    error.value = String(e);
  }
}

function addRule() {
  ts.highlightRules.push({ pattern: "", isRegex: true, bg: "", fg: "#f14c4c" });
}

function removeRule(i: number) {
  ts.highlightRules.splice(i, 1);
}

async function toggleFullscreen() {
  try {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  } catch {
    /* unavailable in plain-browser dev */
  }
}
</script>

<template>
  <div v-if="visible" class="mask" @click.self="visible = false">
    <div class="panel">
      <div class="head">
        <h3>终端设置</h3>
        <a class="x" @click="visible = false">✕</a>
      </div>

      <!-- fonts -->
      <div class="sect">字体与光标</div>
      <div class="grid">
        <label>ASCII 字体</label>
        <input v-model="ts.asciiFont" placeholder="Consolas" />

        <label>中文字体</label>
        <input v-model="ts.cjkFont" placeholder="Microsoft YaHei" />

        <label>字号</label>
        <input v-model.number="ts.fontSize" type="number" min="8" max="40" style="width: 90px" />

        <label>回滚行数</label>
        <input v-model.number="ts.scrollback" type="number" min="100" step="1000" style="width: 120px" />

        <label>光标样式</label>
        <div class="hrow">
          <select v-model="ts.cursorStyle" style="width: 130px">
            <option value="block">方块 block</option>
            <option value="underline">下划线 underline</option>
            <option value="bar">竖线 bar</option>
          </select>
          <label class="chk"><input v-model="ts.cursorBlink" type="checkbox" /> 闪烁</label>
        </div>
      </div>

      <!-- paste throttling -->
      <div class="sect">粘贴限速(大段粘贴分段发送,防止远端丢字符)</div>
      <div class="grid">
        <label>分段大小</label>
        <div class="hrow">
          <input v-model.number="ts.pasteChunkSize" type="number" min="0" step="256" style="width: 120px" />
          <span class="hint">字符/段,0 = 不分段</span>
        </div>

        <label>分段延时</label>
        <div class="hrow">
          <input v-model.number="ts.pasteDelayMs" type="number" min="0" step="10" style="width: 120px" />
          <span class="hint">毫秒</span>
        </div>
      </div>

      <!-- window -->
      <div class="sect">窗口</div>
      <div class="grid">
        <label>不透明度</label>
        <div class="hrow">
          <input
            v-model.number="ts.opacity"
            type="range"
            min="0.4"
            max="1"
            step="0.05"
            style="flex: 1"
          />
          <span class="hint">{{ Math.round(ts.opacity * 100) }}%</span>
        </div>

        <label>全屏</label>
        <div class="hrow">
          <button class="mini" @click="toggleFullscreen">切换全屏 (F11 风格)</button>
        </div>
      </div>

      <!-- theme -->
      <div class="sect">配色主题{{ ts.customTheme ? "(已导入自定义主题)" : "" }}</div>
      <div class="hrow">
        <select
          :value="ts.customTheme ? '__custom__' : ts.themeName"
          style="flex: 1"
          @change="(e) => (e.target as HTMLSelectElement).value !== '__custom__' && ts.setTheme((e.target as HTMLSelectElement).value)"
        >
          <option v-if="ts.customTheme" value="__custom__">自定义(导入)</option>
          <option v-for="t in BUILTIN_THEMES" :key="t.name" :value="t.name">{{ t.name }}</option>
        </select>
        <button class="mini" @click="importTheme">导入 JSON…</button>
        <button class="mini" @click="exportTheme">导出 JSON…</button>
      </div>

      <!-- highlight rules -->
      <div class="sect">关键词高亮(正则匹配渲染行,自动上色)</div>
      <div v-for="(r, i) in ts.highlightRules" :key="i" class="rule">
        <input v-model="r.pattern" class="pat" placeholder="正则或文本,如 \b(error|failed)\b" />
        <label class="chk"><input v-model="r.isRegex" type="checkbox" /> 正则</label>
        <input v-model="r.fg" class="color" type="color" title="前景色(匹配文字)" />
        <label class="chk"><input type="checkbox" :checked="!r.fg" @change="r.fg = ($event.target as HTMLInputElement).checked ? '' : '#f14c4c'" /> 无前景</label>
        <input v-model="r.bg" class="color" type="color" title="背景色" />
        <label class="chk"><input type="checkbox" :checked="!r.bg" @change="r.bg = ($event.target as HTMLInputElement).checked ? '' : '#613214'" /> 无背景</label>
        <button class="mini danger" @click="removeRule(i)">删</button>
      </div>
      <div class="btns-left">
        <button class="mini" @click="addRule">+ 添加规则</button>
      </div>

      <div v-if="error" class="err">{{ error }}</div>
      <div class="btns">
        <span v-if="tip" class="ok">{{ tip }}</span>
        <button class="primary" @click="apply">保存并应用</button>
      </div>
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
  width: 600px;
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
  align-items: center;
}
.hint {
  color: #666;
  font-size: 11px;
  white-space: nowrap;
}
.chk {
  display: flex;
  align-items: center;
  gap: 5px;
  color: #aaa;
  font-size: 12px;
  white-space: nowrap;
}
.chk input {
  width: auto;
}
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 4px 10px;
  cursor: pointer;
  white-space: nowrap;
}
.mini.danger {
  color: #f14c4c;
}
.mini:hover {
  background: #3c3c3c;
}
.rule {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 0;
}
.rule .pat {
  flex: 1;
  font-family: Consolas, monospace;
  font-size: 12px;
}
.rule .color {
  width: 28px;
  height: 26px;
  padding: 1px;
  cursor: pointer;
}
.btns-left {
  margin-top: 8px;
}
.err {
  color: #f14c4c;
  font-size: 12px;
  margin-top: 10px;
}
.ok {
  color: #4ec9b0;
  font-size: 12px;
  margin-right: auto;
}
.btns {
  margin-top: 14px;
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
