<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  externalEditOpen,
  fsDelete,
  fsHome,
  fsList,
  fsMkdir,
  fsRename,
  sftpDelete,
  sftpDownload,
  sftpHome,
  sftpList,
  sftpMkdir,
  sftpRename,
  sftpUpload,
  type FileEntry,
} from "../../ipc/client";
import { trackTransfer } from "../../stores/transfers";
import { sysNotify } from "../../ipc/notify";

const props = defineProps<{
  kind: "local" | "remote";
  sftpId?: string;
  sessionId?: string;
}>();

const path = ref("");
const entries = ref<FileEntry[]>([]);
const loading = ref(false);
const error = ref("");
const selected = ref<FileEntry | null>(null);

const sep = computed(() => (path.value.includes("\\") ? "\\" : "/"));

async function list(p: string) {
  if (props.kind === "remote" && !props.sftpId) return;
  loading.value = true;
  error.value = "";
  try {
    entries.value =
      props.kind === "local" ? await fsList(p) : await sftpList(props.sftpId!, p);
    path.value = p;
    selected.value = null;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function init() {
  try {
    const home = props.kind === "local" ? await fsHome() : await sftpHome(props.sftpId!);
    await list(home);
  } catch (e) {
    error.value = String(e);
  }
}

onMounted(init);
watch(() => props.sftpId, () => init());

function join(dir: string, name: string) {
  return dir.endsWith(sep.value) || dir.endsWith("/") ? dir + name : dir + sep.value + name;
}

function parent(p: string) {
  const trimmed = p.replace(/[\\/]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (i <= 0) return props.kind === "local" && trimmed.includes("\\") ? trimmed.slice(0, 3) : "/";
  return trimmed.slice(0, i);
}

function open(e: FileEntry) {
  if (e.isDir) void list(e.path);
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

async function mkdir() {
  const name = prompt("新建文件夹名称:");
  if (!name) return;
  try {
    if (props.kind === "local") await fsMkdir(join(path.value, name));
    else await sftpMkdir(props.sftpId!, join(path.value, name));
    await list(path.value);
  } catch (e) {
    alert(`创建失败: ${e}`);
  }
}

async function remove() {
  const e = selected.value;
  if (!e) return;
  if (!confirm(`删除 ${e.isDir ? "文件夹" : "文件"} "${e.name}"?`)) return;
  try {
    if (props.kind === "local") await fsDelete(e.path, e.isDir);
    else await sftpDelete(props.sftpId!, e.path, e.isDir);
    await list(path.value);
  } catch (err) {
    alert(`删除失败: ${err}`);
  }
}

async function rename() {
  const e = selected.value;
  if (!e) return;
  const name = prompt("新名称:", e.name);
  if (!name || name === e.name) return;
  try {
    if (props.kind === "local") await fsRename(e.path, join(path.value, name));
    else await sftpRename(props.sftpId!, e.path, join(path.value, name));
    await list(path.value);
  } catch (err) {
    alert(`重命名失败: ${err}`);
  }
}

// ---- drag & drop transfer ----

function onDragStart(e: DragEvent, entry: FileEntry) {
  e.dataTransfer?.setData(
    "application/x-openterm",
    JSON.stringify({ from: props.kind, entry })
  );
}

async function onDrop(e: DragEvent) {
  const raw = e.dataTransfer?.getData("application/x-openterm");
  if (!raw) return;
  const { from, entry } = JSON.parse(raw) as { from: string; entry: FileEntry };
  if (from === props.kind) return; // same side, ignore
  try {
    if (props.kind === "remote" && from === "local") {
      // drop onto remote pane => upload
      const target = join(path.value, entry.name).replace(/\\/g, "/");
      const id = await sftpUpload(props.sftpId!, entry.path, target, false);
      trackTransfer(id, `↑ ${entry.name}`);
    } else if (props.kind === "local" && from === "remote") {
      const target = join(path.value, entry.name);
      const id = await sftpDownload(props.sftpId!, entry.path, target, true);
      trackTransfer(id, `↓ ${entry.name}`);
    }
  } catch (err) {
    alert(`传输失败: ${err}`);
  }
}

// ---- context menu (remote files: external edit) ----

const ctxMenu = ref<{ x: number; y: number; entry: FileEntry } | null>(null);

function onContextMenu(e: MouseEvent, entry: FileEntry) {
  if (props.kind !== "remote" || entry.isDir || !props.sftpId) return;
  e.preventDefault();
  selected.value = entry;
  ctxMenu.value = { x: e.clientX, y: e.clientY, entry };
  const close = () => {
    ctxMenu.value = null;
    window.removeEventListener("click", close);
  };
  window.addEventListener("click", close);
}

async function externalEdit() {
  const m = ctxMenu.value;
  ctxMenu.value = null;
  if (!m || !props.sftpId) return;
  try {
    await externalEditOpen(props.sftpId, m.entry.path);
    void sysNotify("外部编辑", `${m.entry.name} 已用默认程序打开,保存后将自动回传`);
  } catch (err) {
    alert(`外部编辑失败: ${err}`);
  }
}

// ---- explicit transfer buttons (act on selection) ----
const emit = defineEmits<{
  (e: "transfer-out", entry: FileEntry): void;
}>();

defineExpose({ currentPath: () => path.value, refresh: () => list(path.value) });
</script>

<template>
  <div class="panel" @dragover.prevent @drop="onDrop">
    <div class="phead">
      <span class="side">{{ kind === "local" ? "本地" : "远端" }}</span>
      <input
        class="path"
        :value="path"
        @keyup.enter="list(($event.target as HTMLInputElement).value)"
        @change="list(($event.target as HTMLInputElement).value)"
      />
      <button title="上级目录" @click="list(parent(path))">⬆</button>
      <button title="刷新" @click="list(path)">⟳</button>
      <button title="新建文件夹" @click="mkdir">＋</button>
      <button title="重命名" :disabled="!selected" @click="rename">✎</button>
      <button title="删除" :disabled="!selected" @click="remove">🗑</button>
    </div>
    <div class="plist" :class="{ loading }">
      <div v-if="error" class="perr">{{ error }}</div>
      <table v-else>
        <tbody>
          <tr
            v-for="e in entries"
            :key="e.path"
            :class="{ sel: selected?.path === e.path }"
            draggable="true"
            @click="selected = e"
            @dblclick="open(e)"
            @dragstart="onDragStart($event, e)"
            @contextmenu="onContextMenu($event, e)"
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

    <!-- context menu -->
    <div
      v-if="ctxMenu"
      class="ctxmenu"
      :style="{ left: `${ctxMenu.x}px`, top: `${ctxMenu.y}px` }"
    >
      <div class="mi" @click="externalEdit">外部编辑(保存自动回传)</div>
    </div>
  </div>
</template>

<style scoped>
.panel {
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
.perr {
  color: #f77;
  padding: 10px;
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
.ctxmenu {
  position: fixed;
  z-index: 200;
  background: #2d2d30;
  border: 1px solid #454545;
  border-radius: 6px;
  padding: 4px 0;
  min-width: 160px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
}
.ctxmenu .mi {
  padding: 5px 16px;
  font-size: 12px;
  color: #ddd;
  cursor: pointer;
}
.ctxmenu .mi:hover {
  background: #094771;
}
</style>
