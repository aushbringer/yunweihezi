import { defineStore } from "pinia";
import { ref } from "vue";
import {
  credentialList,
  credentialSecret,
  cryptoStatus,
  groupList,
  sessionList,
  sessionOpen,
  sessionClose,
  sftpOpen,
  sftpClose,
  localShellOpen,
  telnetOpen,
  rloginOpen,
  serialOpen,
  tunnelSavedList,
  tunnelStart,
  type AuthMethod,
  type CredentialMeta,
  type Group,
  type OutputEvent,
  type SessionOptions,
  type SessionRecord,
} from "../ipc/client";

export interface Tab {
  id: string; // tab id
  sessionId: string; // backend session id ("" until connected)
  recordId: string | null; // SessionRecord.id if opened from tree
  kind: string; // ssh | telnet | rlogin | serial | local
  title: string;
  state: string; // connecting | connected | disconnected | closed | error
  sftpId: string | null;
  sftpVisible: boolean;
}

export interface ModemStatus {
  active: boolean;
  text: string;
  transferred: number;
  total: number;
  error: string;
}

export const useAppStore = defineStore("app", () => {
  const groups = ref<Group[]>([]);
  const sessions = ref<SessionRecord[]>([]);
  const credentials = ref<CredentialMeta[]>([]);
  const tabs = ref<Tab[]>([]);
  const activeTabId = ref<string | null>(null);

  const masterSet = ref(false);
  const unlocked = ref(false);

  async function refreshCrypto() {
    const s = await cryptoStatus();
    masterSet.value = s.masterSet;
    unlocked.value = s.unlocked;
  }

  async function refreshAll() {
    [groups.value, sessions.value, credentials.value] = await Promise.all([
      groupList(),
      sessionList(),
      credentialList(),
    ]);
  }

  function tabById(id: string | null) {
    return tabs.value.find((t) => t.id === id) ?? null;
  }

  async function resolveAuth(rec: SessionRecord): Promise<AuthMethod> {
    if (!rec.credentialId) return { type: "agent" };
    const meta = credentials.value.find((c) => c.id === rec.credentialId);
    const sec = (await credentialSecret(rec.credentialId)) ?? {};
    switch (meta?.kind) {
      case "password":
        return { type: "password", password: sec.password ?? "" };
      case "keyboardInteractive":
        return { type: "keyboardInteractive", password: sec.password ?? "" };
      case "key":
        return { type: "key", path: sec.keyPath ?? "", passphrase: sec.passphrase };
      default:
        return { type: "agent" };
    }
  }

  const modemStatus = ref<Map<string, ModemStatus>>(new Map());

  // ---------- broadcast input ----------

  /** When true, keyboard/paste input is sent to every included tab. */
  const broadcastEnabled = ref(false);
  /** Tabs excluded from broadcast (all tabs included by default). */
  const broadcastExcluded = ref<Set<string>>(new Set());

  function setBroadcast(v: boolean) {
    broadcastEnabled.value = v;
    if (!v) broadcastExcluded.value = new Set();
  }

  function toggleBroadcastExclude(tabId: string) {
    const s = new Set(broadcastExcluded.value);
    if (s.has(tabId)) s.delete(tabId);
    else s.add(tabId);
    broadcastExcluded.value = s;
  }

  /** Session ids currently receiving broadcast input. */
  function broadcastSessions(): string[] {
    if (!broadcastEnabled.value) return [];
    return tabs.value
      .filter((t) => t.sessionId && !broadcastExcluded.value.has(t.id))
      .map((t) => t.sessionId);
  }


  function handleModemEvent(tabId: string, ev: Extract<OutputEvent, { kind: "modem" }>) {
    const cur =
      modemStatus.value.get(tabId) ?? {
        active: false,
        text: "",
        transferred: 0,
        total: 0,
        error: "",
      };
    switch (ev.event) {
      case "start":
        modemStatus.value.set(tabId, {
          active: true,
          text: ev.message ?? "传输开始",
          transferred: 0,
          total: 0,
          error: "",
        });
        break;
      case "fileStart":
        modemStatus.value.set(tabId, {
          ...cur,
          active: true,
          text: ev.message ?? "",
          transferred: 0,
          total: ev.total ?? 0,
        });
        break;
      case "progress":
        modemStatus.value.set(tabId, {
          ...cur,
          transferred: ev.transferred ?? cur.transferred,
          total: ev.total ?? cur.total,
        });
        break;
      case "fileDone":
        modemStatus.value.set(tabId, { ...cur, text: `已保存: ${ev.message ?? ""}` });
        break;
      case "done":
        modemStatus.value.set(tabId, { ...cur, active: false, text: "传输完成" });
        break;
      case "error":
        modemStatus.value.set(tabId, {
          ...cur,
          active: false,
          error: ev.message ?? "传输失败",
        });
        break;
      case "zrinit":
        // TerminalTabs observes this and opens the file picker.
        modemStatus.value.set(tabId, { ...cur, text: "对端等待接收(rz),请选择文件发送" });
        break;
    }
  }

  /** Create a tab wired to an opener; shared by all protocols. */
  async function openTab(
    kind: string,
    title: string,
    recordId: string | null,
    opener: (onOutput: (ev: OutputEvent) => void) => Promise<string>
  ) {
    const tabId = crypto.randomUUID();
    const tab: Tab = {
      id: tabId,
      sessionId: "",
      recordId,
      kind,
      title,
      state: "connecting",
      sftpId: null,
      sftpVisible: false,
    };
    tabs.value.push(tab);
    activeTabId.value = tabId;

    const onOutput = (ev: OutputEvent) => {
      if (ev.kind === "state") {
        tab.state = ev.state;
        return;
      }
      if (ev.kind === "modem") {
        handleModemEvent(tabId, ev);
        // still forward so the UI (file picker) can react
        outputHandlers.get(tabId)?.(ev);
        return;
      }
      outputHandlers.get(tabId)?.(ev);
    };
    tab.sessionId = await opener(onOutput);
    return tab;
  }

  /** Open a session from a tree record into a new tab. */
  async function openSession(rec: SessionRecord) {
    const opts: SessionOptions = JSON.parse(rec.options || "{}");
    switch (rec.protocol) {
      case "telnet":
        return openTab("telnet", rec.name, rec.id, (onOutput) =>
          telnetOpen({ host: rec.host, port: rec.port, term: opts.term }, onOutput, rec.name, rec.id)
        );
      case "rlogin":
        return openTab("rlogin", rec.name, rec.id, (onOutput) =>
          rloginOpen(
            { host: rec.host, port: rec.port, username: rec.username, term: opts.term },
            onOutput,
            rec.name,
            rec.id
          )
        );
      case "serial":
        return openTab("serial", rec.name, rec.id, (onOutput) =>
          serialOpen({ port: rec.host, ...(opts.serial ?? {}) }, onOutput, rec.name, rec.id)
        );
      case "local":
        return openTab("local", rec.name, rec.id, (onOutput) =>
          localShellOpen(opts.localShell ?? {}, onOutput, rec.name, rec.id)
        );
      default: {
        const auth = await resolveAuth(rec);
        const tab = await openTab("ssh", rec.name, rec.id, (onOutput) =>
          sessionOpen(
            {
              host: rec.host,
              port: rec.port,
              username: rec.username,
              auth,
              jumpChain: opts.jumpChain ?? [],
              proxy: opts.proxy ?? null,
              keepaliveSecs: opts.keepaliveSecs ?? 30,
              reconnect: opts.reconnect ?? false,
              term: opts.term ?? "xterm-256color",
              x11: opts.x11 ?? false,
            },
            onOutput,
            rec.name,
            rec.id
          )
        );
        tab.state = "connected";
        // autostart saved tunnels bound to this record
        if (rec.id) {
          void autostartTunnels(rec.id, tab.sessionId);
        }
        return tab;
      }
    }
  }

  async function autostartTunnels(recordId: string, sessionId: string) {
    try {
      const defs = await tunnelSavedList(recordId);
      for (const d of defs.filter((x) => x.autostart)) {
        tunnelStart({
          sessionId,
          kind: d.kind,
          bindHost: d.listenHost,
          bindPort: d.listenPort,
          targetHost: d.targetHost,
          targetPort: d.targetPort,
        }).catch(() => {});
      }
    } catch {
      /* non-fatal */
    }
  }

  /** Quick ad-hoc connection (not persisted). */
  async function openAdhoc(
    name: string,
    cfg: Parameters<typeof sessionOpen>[0]
  ) {
    const tab = await openTab("ssh", name, null, (onOutput) => sessionOpen(cfg, onOutput, name));
    tab.state = "connected";
    return tab;
  }

  // TerminalTabs registers per-tab output handlers so xterm instances receive
  // only their own session's bytes.
  const outputHandlers = new Map<string, (ev: OutputEvent) => void>();
  function registerOutput(tabId: string, fn: (ev: OutputEvent) => void) {
    outputHandlers.set(tabId, fn);
  }
  function unregisterOutput(tabId: string) {
    outputHandlers.delete(tabId);
  }

  async function closeTab(tabId: string) {
    const tab = tabById(tabId);
    if (!tab) return;
    unregisterOutput(tabId);
    if (tab.sftpId) {
      await sftpClose(tab.sftpId).catch(() => {});
    }
    if (tab.sessionId) {
      await sessionClose(tab.sessionId).catch(() => {});
    }
    tabs.value = tabs.value.filter((t) => t.id !== tabId);
    if (broadcastExcluded.value.has(tabId)) {
      const s = new Set(broadcastExcluded.value);
      s.delete(tabId);
      broadcastExcluded.value = s;
    }
    if (activeTabId.value === tabId) {
      activeTabId.value = tabs.value[tabs.value.length - 1]?.id ?? null;
    }
  }

  /**
   * Remove a tab locally WITHOUT closing the backend session — the session is
   * handed over to a detached window which re-attaches via session_attach.
   */
  function detachTabLocal(tabId: string) {
    const tab = tabById(tabId);
    if (!tab) return;
    unregisterOutput(tabId);
    if (tab.sftpId) {
      void sftpClose(tab.sftpId).catch(() => {});
    }
    tabs.value = tabs.value.filter((t) => t.id !== tabId);
    if (broadcastExcluded.value.has(tabId)) {
      const s = new Set(broadcastExcluded.value);
      s.delete(tabId);
      broadcastExcluded.value = s;
    }
    if (activeTabId.value === tabId) {
      activeTabId.value = tabs.value[tabs.value.length - 1]?.id ?? null;
    }
  }

  async function ensureSftp(tabId: string): Promise<string> {
    const tab = tabById(tabId);
    if (!tab) throw new Error("tab not found");
    if (!tab.sftpId) {
      tab.sftpId = await sftpOpen(tab.sessionId);
    }
    tab.sftpVisible = true;
    return tab.sftpId;
  }

  return {
    groups,
    sessions,
    credentials,
    tabs,
    activeTabId,
    masterSet,
    unlocked,
    modemStatus,
    broadcastEnabled,
    broadcastExcluded,
    setBroadcast,
    toggleBroadcastExclude,
    broadcastSessions,
    refreshCrypto,
    refreshAll,
    openSession,
    openAdhoc,
    openTab,
    closeTab,
    detachTabLocal,
    ensureSftp,
    registerOutput,
    unregisterOutput,
  };
});
