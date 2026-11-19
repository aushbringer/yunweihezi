import { defineStore } from "pinia";
import { ref } from "vue";
import {
  quickCommandList,
  sessionInputText,
  settingGet,
  settingSet,
  type QuickCommand,
} from "../ipc/client";
import { useAppStore } from "./app";

export type ToolbarAction =
  | "newSession"
  | "sftp"
  | "tunnels"
  | "logs"
  | "quickcmd"
  | "triggers"
  | "macros"
  | "termSettings"
  | "ftp"
  | "lock";

export type ToolbarItem =
  | { kind: "action"; action: ToolbarAction }
  | { kind: "qcmd"; id: string };

export const ACTION_LABELS: Record<ToolbarAction, string> = {
  newSession: "新建会话",
  sftp: "SFTP",
  tunnels: "隧道",
  logs: "日志",
  quickcmd: "快捷命令",
  triggers: "触发器",
  macros: "键盘宏",
  termSettings: "终端",
  ftp: "FTP",
  lock: "锁定",
};

/** Default toolbar layout. Note: FTP 入口默认隐藏(不在默认布局中),可由用户添加。 */
const DEFAULT_ITEMS: ToolbarItem[] = [
  { kind: "action", action: "newSession" },
  { kind: "action", action: "sftp" },
  { kind: "action", action: "tunnels" },
  { kind: "action", action: "logs" },
  { kind: "action", action: "quickcmd" },
  { kind: "action", action: "triggers" },
  { kind: "action", action: "macros" },
  { kind: "action", action: "termSettings" },
  { kind: "action", action: "lock" },
];

/** Customizable top toolbar layout, persisted as JSON in the settings table. */
export const useToolbarStore = defineStore("toolbar", () => {
  const items = ref<ToolbarItem[]>([...DEFAULT_ITEMS]);
  const qcmds = ref<QuickCommand[]>([]);
  const loaded = ref(false);

  async function load() {
    const [raw, qc] = await Promise.all([
      settingGet("toolbar.items"),
      quickCommandList().catch(() => [] as QuickCommand[]),
    ]);
    qcmds.value = qc;
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed) && parsed.every((x) => x && typeof x.kind === "string")) {
          items.value = parsed;
        }
      } catch {
        /* keep defaults */
      }
    }
    loaded.value = true;
  }

  async function refreshQcmds() {
    qcmds.value = await quickCommandList().catch(() => []);
  }

  async function save() {
    await settingSet("toolbar.items", JSON.stringify(items.value));
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= items.value.length) return;
    const arr = [...items.value];
    [arr[i], arr[j]] = [arr[j], arr[i]];
    items.value = arr;
    await save().catch(() => {});
  }

  async function removeAt(i: number) {
    items.value = items.value.filter((_, x) => x !== i);
    await save().catch(() => {});
  }

  async function add(item: ToolbarItem) {
    items.value = [...items.value, item];
    await save().catch(() => {});
  }

  async function reset() {
    items.value = [...DEFAULT_ITEMS];
    await save().catch(() => {});
  }

  function qcmdById(id: string) {
    return qcmds.value.find((q) => q.id === id) ?? null;
  }

  /** Run a toolbar quick command against the active session. */
  async function runQcmd(id: string) {
    const cmd = qcmdById(id);
    const app = useAppStore();
    const sid = app.tabs.find((t) => t.id === app.activeTabId)?.sessionId;
    if (!cmd || !sid) return;
    let text = cmd.content.replace(/\r?\n/g, "\r");
    if (!text.endsWith("\r")) text += "\r";
    await sessionInputText(sid, text);
  }

  return { items, qcmds, loaded, load, refreshQcmds, save, move, removeAt, add, reset, qcmdById, runQcmd };
});
