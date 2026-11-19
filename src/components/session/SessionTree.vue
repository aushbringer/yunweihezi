<script setup lang="ts">
import { computed, ref } from "vue";
import { useAppStore } from "../../stores/app";
import {
  groupDelete,
  groupUpsert,
  sessionDelete,
  sessionUpsert,
  configExport,
  configImport,
} from "../../ipc/client";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { textFileRead, textFileWrite } from "../../ipc/files";

const emit = defineEmits<{
  (e: "new-session"): void;
  (e: "edit-session", id: string): void;
}>();

const store = useAppStore();
const filter = ref("");

interface TreeNode {
  group: import("../../ipc/client").Group | null;
  sessions: import("../../ipc/client").SessionRecord[];
  children: TreeNode[];
}

const tree = computed<TreeNode>(() => {
  const f = filter.value.trim().toLowerCase();
  const match = (s: import("../../ipc/client").SessionRecord) =>
    !f || s.name.toLowerCase().includes(f) || s.host.toLowerCase().includes(f);
  const build = (parentId: string | null): TreeNode[] =>
    store.groups
      .filter((g) => g.parentId === parentId)
      .sort((a, b) => a.sort - b.sort || a.name.localeCompare(b.name))
      .map((g) => ({
        group: g,
        sessions: store.sessions.filter((s) => s.groupId === g.id && match(s)),
        children: build(g.id),
      }));
  return {
    group: null,
    sessions: store.sessions.filter((s) => !s.groupId && match(s)),
    children: build(null),
  };
});

const collapsed = ref(new Set<string>());
function toggle(id: string) {
  collapsed.value.has(id) ? collapsed.value.delete(id) : collapsed.value.add(id);
  collapsed.value = new Set(collapsed.value);
}

async function connect(rec: import("../../ipc/client").SessionRecord) {
  if (!store.unlocked && rec.credentialId) {
    alert("保险库已锁定，请先解锁");
    return;
  }
  try {
    await store.openSession(rec);
  } catch (e) {
    alert(`连接失败: ${e}`);
  }
}

async function newGroup() {
  const name = prompt("分组名称:");
  if (!name) return;
  await groupUpsert({ name });
  await store.refreshAll();
}

// ---------- drag & drop (move session into a group) ----------

const dragOver = ref("");

function onDragStart(s: import("../../ipc/client").SessionRecord, ev: DragEvent) {
  ev.dataTransfer?.setData("text/plain", s.id);
  if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "move";
}

async function onDropToGroup(groupId: string | null, ev: DragEvent) {
  dragOver.value = "";
  const id = ev.dataTransfer?.getData("text/plain");
  if (!id) return;
  const rec = store.sessions.find((s) => s.id === id);
  if (!rec || (rec.groupId ?? null) === groupId) return;
  try {
    await sessionUpsert({
      id: rec.id,
      groupId,
      name: rec.name,
      protocol: rec.protocol,
      host: rec.host,
      port: rec.port,
      username: rec.username,
      credentialId: rec.credentialId,
      options: rec.options,
      sort: rec.sort,
    });
    await store.refreshAll();
  } catch (e) {
    alert(`移动失败: ${e}`);
  }
}

async function removeSession(rec: import("../../ipc/client").SessionRecord) {
  if (!confirm(`删除会话 "${rec.name}"?`)) return;
  await sessionDelete(rec.id);
  await store.refreshAll();
}

async function removeGroup(g: import("../../ipc/client").Group) {
  if (!confirm(`删除分组 "${g.name}"?(组内会话保留为未分组)`)) return;
  await groupDelete(g.id);
  await store.refreshAll();
}

async function doExport() {
  const password = prompt("导出密码(至少 6 位,用于加密导出文件):");
  if (!password) return;
  const path = await save({ defaultPath: "openterm-export.json" });
  if (!path) return;
  try {
    const content = await configExport(password);
    await textFileWrite(path, content);
    alert("导出成功");
  } catch (e) {
    alert(`导出失败: ${e}`);
  }
}

async function doImport() {
  const path = await openDialog({
    filters: [{ name: "OpenTerm 导出", extensions: ["json"] }],
  });
  if (!path || typeof path !== "string") return;
  const password = prompt("导入密码:");
  if (!password) return;
  try {
    const content = await textFileRead(path);
    const report = await configImport(content, password);
    await store.refreshAll();
    alert(`导入完成: 分组 ${report.groups}, 会话 ${report.sessions}, 凭证 ${report.credentials}`);
  } catch (e) {
    alert(`导入失败: ${e}`);
  }
}
</script>

<template>
  <div class="tree-wrap">
    <div class="tree-toolbar">
      <input v-model="filter" class="filter" placeholder="搜索会话…" />
    </div>
    <div class="tree-actions">
      <button title="新建会话" @click="emit('new-session')">＋会话</button>
      <button title="新建分组" @click="newGroup">＋分组</button>
      <button title="导入" @click="doImport">导入</button>
      <button title="导出" @click="doExport">导出</button>
    </div>
    <div class="tree" @dragover.prevent @drop.prevent="onDropToGroup(null, $event)">
      <template v-for="node in tree.children" :key="node.group!.id">
        <div
          class="group-row"
          :class="{ 'drop-hl': dragOver === node.group!.id }"
          @dragover.prevent="dragOver = node.group!.id"
          @dragleave="dragOver = ''"
          @drop.prevent.stop="onDropToGroup(node.group!.id, $event)"
        >
          <span class="twisty" @click="toggle(node.group!.id)">
            {{ collapsed.has(node.group!.id) ? "▸" : "▾" }}
          </span>
          <span class="gname">📁 {{ node.group!.name }}</span>
          <span class="row-ops">
            <a @click="removeGroup(node.group!)" title="删除分组">✕</a>
          </span>
        </div>
        <div
          v-if="!collapsed.has(node.group!.id)"
          class="group-body"
          :class="{ 'drop-hl': dragOver === node.group!.id }"
          @dragover.prevent="dragOver = node.group!.id"
          @dragleave="dragOver = ''"
          @drop.prevent.stop="onDropToGroup(node.group!.id, $event)"
        >
          <div
            v-for="s in node.sessions"
            :key="s.id"
            class="session-row"
            draggable="true"
            @dragstart="onDragStart(s, $event)"
            @dblclick="connect(s)"
          >
            <span class="sicon">🖥</span>
            <span class="sname" :title="`${s.username}@${s.host}:${s.port}`">{{ s.name }}</span>
            <span class="row-ops">
              <a @click.stop="connect(s)" title="连接">▶</a>
              <a @click.stop="emit('edit-session', s.id)" title="编辑">✎</a>
              <a @click.stop="removeSession(s)" title="删除">✕</a>
            </span>
          </div>
          <template v-for="child in node.children" :key="child.group!.id">
            <!-- nested groups rendered flat-indented for simplicity -->
            <div
              class="group-row indent"
              :class="{ 'drop-hl': dragOver === child.group!.id }"
              @dragover.prevent.stop="dragOver = child.group!.id"
              @dragleave="dragOver = ''"
              @drop.prevent.stop="onDropToGroup(child.group!.id, $event)"
            >
              <span class="gname">📁 {{ child.group!.name }}</span>
              <span class="row-ops">
                <a @click="removeGroup(child.group!)" title="删除分组">✕</a>
              </span>
            </div>
            <div
              v-for="s in child.sessions"
              :key="s.id"
              class="session-row indent2"
              draggable="true"
              @dragstart="onDragStart(s, $event)"
              @dblclick="connect(s)"
            >
              <span class="sicon">🖥</span>
              <span class="sname" :title="`${s.username}@${s.host}:${s.port}`">{{ s.name }}</span>
              <span class="row-ops">
                <a @click.stop="connect(s)" title="连接">▶</a>
                <a @click.stop="emit('edit-session', s.id)" title="编辑">✎</a>
                <a @click.stop="removeSession(s)" title="删除">✕</a>
              </span>
            </div>
          </template>
        </div>
      </template>

      <div v-if="tree.sessions.length" class="group-row">
        <span class="gname">未分组</span>
      </div>
      <div
        v-for="s in tree.sessions"
        :key="s.id"
        class="session-row"
        draggable="true"
        @dragstart="onDragStart(s, $event)"
        @dblclick="connect(s)"
      >
        <span class="sicon">🖥</span>
        <span class="sname" :title="`${s.username}@${s.host}:${s.port}`">{{ s.name }}</span>
        <span class="row-ops">
          <a @click.stop="connect(s)" title="连接">▶</a>
          <a @click.stop="emit('edit-session', s.id)" title="编辑">✎</a>
          <a @click.stop="removeSession(s)" title="删除">✕</a>
        </span>
      </div>

      <div v-if="!tree.sessions.length && !tree.children.length" class="empty">
        暂无会话,点击「＋会话」创建
      </div>
    </div>
  </div>
</template>

<style scoped>
.tree-wrap {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
}
.tree-toolbar {
  padding: 6px;
}
.filter {
  width: 100%;
  background: #191919;
  border: 1px solid #3c3c3c;
  color: #ddd;
  padding: 4px 8px;
  border-radius: 4px;
  box-sizing: border-box;
}
.tree-actions {
  display: flex;
  gap: 4px;
  padding: 0 6px 6px;
}
.tree-actions button {
  flex: 1;
  background: #37373d;
  border: 1px solid #4a4a4a;
  color: #ccc;
  font-size: 11px;
  padding: 3px 0;
  border-radius: 3px;
  cursor: pointer;
}
.tree-actions button:hover {
  background: #454549;
}
.tree {
  flex: 1;
  overflow-y: auto;
  padding-bottom: 12px;
}
.group-row,
.session-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  cursor: default;
  user-select: none;
}
.session-row {
  cursor: pointer;
  padding-left: 22px;
}
.session-row:hover,
.group-row:hover {
  background: #2d2d30;
}
.session-row[draggable="true"] {
  cursor: grab;
}
.drop-hl {
  outline: 1px dashed #0e639c;
  outline-offset: -1px;
  background: rgba(14, 99, 156, 0.12);
}
.session-row.indent2 {
  padding-left: 34px;
}
.group-row.indent {
  padding-left: 22px;
}
.twisty {
  width: 14px;
  color: #999;
  cursor: pointer;
}
.gname {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #bbb;
  font-size: 12px;
}
.sname {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12.5px;
}
.row-ops {
  display: none;
  gap: 6px;
}
.session-row:hover .row-ops,
.group-row:hover .row-ops {
  display: inline-flex;
}
.row-ops a {
  color: #999;
  cursor: pointer;
  font-size: 11px;
}
.row-ops a:hover {
  color: #fff;
}
.empty {
  padding: 20px 12px;
  color: #777;
  font-size: 12px;
  text-align: center;
}
</style>
