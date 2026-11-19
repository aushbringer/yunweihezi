<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ftpClose,
  ftpDelete,
  ftpDownload,
  ftpList,
  ftpMkdir,
  ftpOpen,
  ftpRename,
  ftpUpload,
  type FileEntry,
} from "../../ipc/client";
import SftpPanel from "../sftp/SftpPanel.vue";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit("update:modelValue", v),
});

const form = reactive({
  host: "",
  port: 21,
  username: "",
  password: "",
  anonymous: false,
});

const ftpId = ref("");
const connecting = ref(false);
const status = ref("");
const statusErr = ref(false);

const localPanel = ref<{ currentPath: () => string; refresh: () => Promise<void> } | null>(null);

// ---- remote panel state ----
const path = ref("/");
const entries = ref<FileEntry[]>([]);
const loading = ref(false);
const selected = ref<FileEntry | null>(null);

function setStatus(msg: string, isErr = false) {
  status.value = msg;
  statusErr.value = isErr;
}

async function connect() {
  if (!form.host) {
    setStatus("请填写主机", true);
    return;
  }
  connecting.value = true;
  setStatus("连接中…");
  try {
    ftpId.value = await ftpOpen({
      host: form.host,
      port: form.port || 21,
      username: form.anonymous ? "" : form.username,
      password: form.anonymous ? "" : form.password,
    });
    setStatus("已连接");
    await list("/");
  } catch (e) {
    setStatus(`连接失败: ${e}`, true);
  } finally {
    connecting.value = false;
  }
}

async function disconnect() {
  if (ftpId.value) {
    const id = ftpId.value;
    ftpId.value = "";
    await ftpClose(id).catch(() => {});
  }
  entries.value = [];
  selected.value = null;
  path.value = "/";
  setStatus("");
}

async function list(p: string) {
  if (!ftpId.value) return;
  loading.value = true;
  try {
    entries.value = await ftpList(ftpId.value, p);
    path.value = p;
    selected.value = null;
  } catch (e) {
    setStatus(`列目录失败: ${e}`, true);
  } finally {
    loading.value = false;
  }
}

function join(dir: string, name: string) {
  return dir.endsWith("/") ? dir + name : dir + "/" + name;
}

function parent(p: string) {
  const trimmed = p.replace(/\/+$/, "");
  const i = trimmed.lastIndexOf("/");
  return i <= 0 ? "/" : trimmed.slice(0, i);
}

function localJoin(dir: string, name: string) {
  const sep = dir.includes("\\") ? "\\" : "/";
  return dir.endsWith(sep) || dir.endsWith("/") ? dir + name : dir + sep + name;
}

function localCwd() {
  return localPanel.value?.currentPath() || "";
}

function openEntry(e: FileEntry) {
  if (e.isDir) void list(e.path);
  else void download(e);
}

async function refresh() {
  await list(path.value);
}

async function mkdir() {
  const name = prompt("新建文件夹名称:");
  if (!name || !ftpId.value) return;
  try {
    await ftpMkdir(ftpId.value, join(path.value, name));
    await refresh();
    setStatus(`已创建 ${name}`);
  } catch (e) {
    setStatus(`创建失败: ${e}`, true);
  }
}

async function remove() {
  const e = selected.value;
  if (!e || !ftpId.value) return;
  if (!confirm(`删除${e.isDir ? "文件夹" : "文件"} "${e.name}"?`)) return;
  try {
    await ftpDelete(ftpId.value, e.path, e.isDir);
    await refresh();
    setStatus(`已删除 ${e.name}`);
  } catch (err) {
    setStatus(`删除失败: ${err}`, true);
  }
}

async function rename() {
  const e = selected.value;
  if (!e || !ftpId.value) return;
  const name = prompt("新名称:", e.name);
  if (!name || name === e.name) return;
  try {
    await ftpRename(ftpId.value, e.path, join(path.value, name));
    await refresh();
    setStatus(`已重命名为 ${name}`);
  } catch (err) {
    setStatus(`重命名失败: ${err}`, true);
  }
}

async function upload() {
  if (!ftpId.value) return;
  const picked = await openDialog({ multiple: false });
  if (!picked || typeof picked !== "string") return;
  const name = picked.split(/[\\/]/).pop()!;
  setStatus(`上传中 ${name} …`);
  try {
    await ftpUpload(ftpId.value, picked, join(path.value, name));
    await refresh();
    setStatus(`上传完成 ${name}`);
  } catch (e) {
    setStatus(`上传失败: ${e}`, true);
  }
}

async function download(e?: FileEntry) {
  const entry = e ?? selected.value;
  if (!entry || entry.isDir || !ftpId.value) return;
  const cwd = localCwd();
  if (!cwd) {
    setStatus("本地面板目录不可用", true);
    return;
  }
  setStatus(`下载中 ${entry.name} …`);
  try {
    await ftpDownload(ftpId.value, entry.path, localJoin(cwd, entry.name));
    await localPanel.value?.refresh();
    setStatus(`下载完成 ${entry.name}`);
  } catch (err) {
    setStatus(`下载失败: ${err}`, true);
  }
}

function fmtSize(n: number) {
  if (n < 1024) return `${n}`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)}M`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)}G`;
}

function fmtTime(t: number) {
  if (!t) return "";
  return new Date(t * 1000).toLocaleString();
}

watch(visible, (v) => {
  if (!v) void disconnect();
});

onBeforeUnmount(() => void disconnect());
</script>

<template>
  <div v-if="visible" class="mask" @click.self="visible = false">
    <div class="panel" :class="{ wide: !!ftpId }">
      <div class="head">
        <h3>FTP（明文）</h3>
        <div class="hbtns">
          <button v-if="ftpId" class="mini" @click="disconnect">断开</button>
          <a class="x" @click="visible = false">✕</a>
        </div>
      </div>

      <!-- connect form -->
      <template v-if="!ftpId">
        <div class="grid">
          <label>主机</label>
          <input v-model="form.host" placeholder="ftp.example.com" @keyup.enter="connect" />
          <label>端口</label>
          <input v-model.number="form.port" type="number" @keyup.enter="connect" />
          <template v-if="!form.anonymous">
            <label>用户名</label>
            <input v-model="form.username" @keyup.enter="connect" />
            <label>密码</label>
            <input v-model="form.password" type="password" @keyup.enter="connect" />
          </template>
          <label>选项</label>
          <label class="chk"><input v-model="form.anonymous" type="checkbox" /> 匿名登录</label>
        </div>
        <div class="btns">
          <button class="primary" :disabled="connecting" @click="connect">
            {{ connecting ? "连接中…" : "连接" }}
          </button>
        </div>
      </template>

      <!-- dual-pane file manager -->
      <div v-else class="panes">
        <SftpPanel ref="localPanel" kind="local" />
        <div class="panel-r">
          <div class="phead">
            <span class="side">FTP</span>
            <input
              class="path"
              :value="path"
              @keyup.enter="list(($event.target as HTMLInputElement).value)"
            />
            <button title="上级目录" @click="list(parent(path))">⬆</button>
            <button title="刷新" @click="refresh">⟳</button>
            <button title="新建文件夹" @click="mkdir">＋</button>
            <button title="重命名" :disabled="!selected" @click="rename">✎</button>
            <button title="删除" :disabled="!selected" @click="remove">🗑</button>
            <button title="上传" @click="upload">⇧</button>
            <button title="下载" :disabled="!selected || selected.isDir" @click="download()">⇩</button>
          </div>
          <div class="plist" :class="{ loading }">
            <table>
              <tbody>
                <tr
                  v-for="e in entries"
                  :key="e.path"
                  :class="{ sel: selected?.path === e.path }"
                  @click="selected = e"
                  @dblclick="openEntry(e)"
                >
                  <td class="name">
                    <span class="ico">{{ e.isDir ? "📁" : "📄" }}</span>
                    {{ e.name }}
                  </td>
                  <td class="size">{{ e.isDir ? "" : fmtSize(e.size) }}</td>
                  <td class="time">{{ fmtTime(e.mtime) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>

      <div v-if="status" class="status" :class="{ err: statusErr }">{{ status }}</div>
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
  width: 420px;
  max-height: 85vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: #252526;
  border: 1px solid #3c3c3c;
  border-radius: 8px;
  padding: 16px 18px;
  color: #ddd;
  font-size: 13px;
}
.panel.wide {
  width: 900px;
  max-width: 92vw;
  height: 70vh;
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
.hbtns {
  display: flex;
  align-items: center;
  gap: 10px;
}
.x {
  cursor: pointer;
  color: #999;
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
.chk {
  display: flex;
  align-items: center;
  gap: 5px;
  color: #aaa;
  font-size: 12px;
  justify-self: start;
}
.chk input {
  width: auto;
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
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 2px 10px;
  cursor: pointer;
}
.mini:hover {
  background: #3c3c3c;
}
.panes {
  flex: 1;
  display: flex;
  gap: 10px;
  min-height: 0;
}
.panel-r {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
.phead {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px;
  background: #232324;
  border-bottom: 1px solid #333;
}
.side {
  font-size: 11px;
  color: #4fc3f7;
  border: 1px solid #4fc3f755;
  border-radius: 3px;
  padding: 1px 6px;
}
.path {
  flex: 1;
  background: #191919;
  border: 1px solid #3c3c3c;
  color: #ccc;
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 3px;
  min-width: 0;
}
.phead button {
  background: #37373d;
  border: 1px solid #4a4a4a;
  color: #ccc;
  border-radius: 3px;
  cursor: pointer;
  padding: 2px 8px;
  font-size: 12px;
}
.phead button:disabled {
  opacity: 0.35;
  cursor: default;
}
.plist {
  flex: 1;
  overflow-y: auto;
  font-size: 12px;
}
.plist.loading {
  opacity: 0.5;
}
table {
  width: 100%;
  border-collapse: collapse;
}
tr {
  cursor: default;
  user-select: none;
}
tr:hover {
  background: #2a2d2e;
}
tr.sel {
  background: #094771;
}
td {
  padding: 2px 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.name {
  max-width: 220px;
}
.ico {
  margin-right: 4px;
}
.size {
  width: 70px;
  text-align: right;
  color: #999;
}
.time {
  width: 150px;
  color: #777;
  font-size: 11px;
}
.status {
  margin-top: 10px;
  font-size: 12px;
  color: #4ec9b0;
}
.status.err {
  color: #f14c4c;
}
</style>
