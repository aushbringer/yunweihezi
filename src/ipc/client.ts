import { invoke, Channel } from "@tauri-apps/api/core";

// ---------- session byte stream ----------

export type OutputEvent =
  | { kind: "data"; data: string } // base64 payload
  | { kind: "state"; state: string; message?: string }
  | { kind: "exit"; code?: number }
  | {
      kind: "modem";
      event: string; // zrinit | start | fileStart | progress | fileDone | done | error
      message?: string;
      transferred?: number;
      total?: number;
    }
  | {
      kind: "trigger";
      name: string;
      action: string; // highlight | alert (send / script run backend-side)
      param: string;
      matched: string;
    };

export type ProxyConfig =
  | { type: "socks5"; host: string; port: number }
  | { type: "socks4"; host: string; port: number }
  | { type: "http"; host: string; port: number };

export type AuthMethod =
  | { type: "password"; password: string }
  | { type: "keyboardInteractive"; password: string }
  | { type: "key"; path: string; passphrase?: string }
  | { type: "agent" };

export interface JumpHop {
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
}

export interface SshSessionConfig {
  host: string;
  port?: number;
  username: string;
  auth: AuthMethod;
  jumpChain?: JumpHop[];
  proxy?: ProxyConfig | null;
  term?: string;
  cols?: number;
  rows?: number;
  keepaliveSecs?: number;
  reconnect?: boolean;
  /** X11 forwarding (needs a local X server: VcXsrv/Xming/XQuartz). */
  x11?: boolean;
}

/** Extra per-session options persisted in SessionRecord.options JSON. */
export interface SessionOptions {
  jumpChain?: JumpHop[];
  proxy?: ProxyConfig | null;
  keepaliveSecs?: number;
  reconnect?: boolean;
  term?: string;
  x11?: boolean;
  /** serial protocol params */
  serial?: Omit<SerialConfig, "port">;
  /** local shell params */
  localShell?: { shell: string; cwd?: string };
}

// ---------- other protocols ----------

export interface LocalShellConfig {
  shell?: string; // cmd | powershell | pwsh | wsl | bash | path
  cwd?: string;
  cols?: number;
  rows?: number;
}

export interface TelnetConfig {
  host: string;
  port?: number;
  term?: string;
  cols?: number;
  rows?: number;
}

export interface RloginConfig {
  host: string;
  port?: number;
  username: string;
  term?: string;
}

export interface SerialConfig {
  port: string;
  baudRate?: number;
  dataBits?: number;
  stopBits?: number;
  parity?: string; // none | odd | even
  flowControl?: string; // none | hardware | software
  downloadDir?: string;
}

export interface SerialPortInfo {
  name: string;
  description: string;
}

export type XyVariant = "XModem" | "XModemCrc" | "XModem1K" | "YModem";

export function localShellOpen(
  config: LocalShellConfig,
  onOutput: (ev: OutputEvent) => void,
  label?: string,
  recordId?: string
): Promise<string> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke<string>("local_shell_open", {
    config,
    label: label ?? null,
    recordId: recordId ?? null,
    onOutput: channel,
  });
}

export function telnetOpen(
  config: TelnetConfig,
  onOutput: (ev: OutputEvent) => void,
  label?: string,
  recordId?: string
): Promise<string> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke<string>("telnet_open", {
    config,
    label: label ?? null,
    recordId: recordId ?? null,
    onOutput: channel,
  });
}

export function rloginOpen(
  config: RloginConfig,
  onOutput: (ev: OutputEvent) => void,
  label?: string,
  recordId?: string
): Promise<string> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke<string>("rlogin_open", {
    config,
    label: label ?? null,
    recordId: recordId ?? null,
    onOutput: channel,
  });
}

export function serialOpen(
  config: SerialConfig,
  onOutput: (ev: OutputEvent) => void,
  label?: string,
  recordId?: string
): Promise<string> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke<string>("serial_open", {
    config,
    label: label ?? null,
    recordId: recordId ?? null,
    onOutput: channel,
  });
}

export const serialListPorts = () => invoke<SerialPortInfo[]>("serial_list_ports");
export const serialZsend = (sessionId: string, paths: string[]) =>
  invoke("serial_zsend", { sessionId, paths });
export const serialXsend = (sessionId: string, path: string, variant: XyVariant) =>
  invoke("serial_xsend", { sessionId, path, variant });
export const serialXrecv = (sessionId: string, variant: XyVariant) =>
  invoke("serial_xrecv", { sessionId, variant });

// ---------- port forwarding ----------

export interface RunningTunnel {
  id: string;
  sessionId: string;
  kind: string; // local | remote | dynamic
  bindHost: string;
  bindPort: number;
  targetHost: string;
  targetPort: number;
}

export interface TunnelRecord {
  id: string;
  sessionId: string | null;
  name: string;
  kind: string;
  listenHost: string;
  listenPort: number;
  targetHost: string;
  targetPort: number;
  autostart: boolean;
}

export interface TunnelStartArgs {
  sessionId: string;
  kind: string;
  bindHost: string;
  bindPort: number;
  targetHost?: string;
  targetPort?: number;
}

export const tunnelStart = (args: TunnelStartArgs) =>
  invoke<RunningTunnel>("tunnel_start", { args });
export const tunnelStop = (id: string) => invoke("tunnel_stop", { id });
export const tunnelRunning = (sessionId?: string) =>
  invoke<RunningTunnel[]>("tunnel_running", { sessionId: sessionId ?? null });
export const tunnelSavedList = (sessionId?: string) =>
  invoke<TunnelRecord[]>("tunnel_saved_list", { sessionId: sessionId ?? null });
export const tunnelSavedUpsert = (record: TunnelRecord) =>
  invoke<TunnelRecord>("tunnel_saved_upsert", { record });
export const tunnelSavedDelete = (id: string) => invoke("tunnel_saved_delete", { id });

const b64encode = (bytes: Uint8Array): string => {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
};

export const b64decode = (b64: string): Uint8Array => {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};

const encoder = new TextEncoder();

export async function sessionOpen(
  config: SshSessionConfig,
  onOutput: (ev: OutputEvent) => void,
  label?: string,
  recordId?: string
): Promise<string> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke<string>("session_open", {
    config,
    label: label ?? null,
    recordId: recordId ?? null,
    onOutput: channel,
  });
}

/** Re-attach a running session's output stream to this window (detached window). */
export function sessionAttach(
  sessionId: string,
  onOutput: (ev: OutputEvent) => void
): Promise<void> {
  const channel = new Channel<OutputEvent>();
  channel.onmessage = onOutput;
  return invoke("session_attach", { sessionId, onOutput: channel });
}

/** Test an SSH connection without saving; resolves with a summary, rejects with the reason. */
export const sshTestConnect = (config: SshSessionConfig) =>
  invoke<string>("ssh_test_connect", { config });

export async function sessionInputText(sessionId: string, text: string): Promise<void> {
  return sessionInputBytes(sessionId, encoder.encode(text));
}

export async function sessionInputBytes(sessionId: string, bytes: Uint8Array): Promise<void> {
  return invoke("session_input", { sessionId, dataB64: b64encode(bytes) });
}

export async function sessionResize(sessionId: string, cols: number, rows: number): Promise<void> {
  return invoke("session_resize", { sessionId, cols, rows });
}

export async function sessionClose(sessionId: string): Promise<void> {
  return invoke("session_close", { sessionId });
}

// ---------- crypto / vault / credentials ----------

export interface CryptoStatus {
  masterSet: boolean;
  unlocked: boolean;
}

export interface CredentialMeta {
  id: string;
  name: string;
  kind: string; // password | key | agent | keyboardInteractive
  username: string;
  hasSecret: boolean;
}

export interface CredentialSecret {
  password?: string;
  keyPath?: string;
  passphrase?: string;
}

export interface CredentialInput {
  name: string;
  kind: string;
  username?: string;
  secret?: CredentialSecret | null;
}

export const cryptoStatus = () => invoke<CryptoStatus>("crypto_status");
export const cryptoSetMaster = (password: string) => invoke("crypto_set_master", { password });
export const cryptoUnlock = (password: string) => invoke("crypto_unlock", { password });
export const cryptoLock = () => invoke("crypto_lock");

export const credentialList = () => invoke<CredentialMeta[]>("credential_list");
export const credentialCreate = (input: CredentialInput) =>
  invoke<string>("credential_create", { input });
export const credentialUpdate = (id: string, input: CredentialInput) =>
  invoke("credential_update", { id, input });
export const credentialSecret = (id: string) =>
  invoke<CredentialSecret | null>("credential_secret", { id });
export const credentialDelete = (id: string) => invoke("credential_delete", { id });

// ---------- config: groups / sessions / settings / import-export ----------

export interface Group {
  id: string;
  name: string;
  parentId: string | null;
  sort: number;
}

export interface SessionRecord {
  id: string;
  groupId: string | null;
  name: string;
  protocol: string;
  host: string;
  port: number;
  username: string;
  credentialId: string | null;
  options: string; // JSON of SessionOptions
  sort: number;
}

export interface SessionInput {
  id?: string;
  groupId?: string | null;
  name: string;
  protocol?: string;
  host?: string;
  port?: number;
  username?: string;
  credentialId?: string | null;
  options?: string;
  sort?: number;
}

export const groupList = () => invoke<Group[]>("group_list");
export const groupUpsert = (input: { id?: string; name: string; parentId?: string | null; sort?: number }) =>
  invoke<string>("group_upsert", { input });
export const groupDelete = (id: string) => invoke("group_delete", { id });

export const sessionList = () => invoke<SessionRecord[]>("session_list");
export const sessionUpsert = (input: SessionInput) => invoke<string>("session_upsert", { input });
export const sessionDelete = (id: string) => invoke("session_delete", { id });

export const settingGet = (key: string) => invoke<string | null>("setting_get", { key });
export const settingSet = (key: string, value: string) => invoke("setting_set", { key, value });

// ---------- quick commands ----------

export interface QuickCommand {
  id: string;
  label: string;
  content: string;
  group: string;
  sort: number;
}

export interface QuickCommandInput {
  id?: string;
  label: string;
  content?: string;
  group?: string;
  sort?: number;
}

export const quickCommandList = () => invoke<QuickCommand[]>("quick_command_list");
export const quickCommandUpsert = (input: QuickCommandInput) =>
  invoke<string>("quick_command_upsert", { input });
export const quickCommandDelete = (id: string) => invoke("quick_command_delete", { id });

// ---------- triggers ----------

export interface TriggerRule {
  id: string;
  sessionId: string | null;
  name: string;
  pattern: string;
  isRegex: boolean;
  /** highlight | alert | send | script */
  action: string;
  actionParam: string;
  enabled: boolean;
}

export interface TriggerInput {
  id?: string;
  sessionId?: string | null;
  name: string;
  pattern: string;
  isRegex?: boolean;
  action: string;
  actionParam?: string;
  enabled?: boolean;
}

export const triggerList = () => invoke<TriggerRule[]>("trigger_list");
export const triggerUpsert = (input: TriggerInput) => invoke<string>("trigger_upsert", { input });
export const triggerDelete = (id: string) => invoke("trigger_delete", { id });

export const configExport = (password: string) => invoke<string>("config_export", { password });
export const configImport = (content: string, password: string) =>
  invoke<{ groups: number; sessions: number; credentials: number }>("config_import", {
    content,
    password,
  });

// ---------- SFTP ----------

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink: boolean;
  size: number;
  mtime: number;
  permissions?: number;
}

export type TransferEvent =
  | { kind: "progress"; id: string; transferred: number; total: number }
  | { kind: "state"; id: string; state: string; message?: string };

export const sftpOpen = (sessionId: string) => invoke<string>("sftp_open", { sessionId });
export const sftpClose = (sftpId: string) => invoke("sftp_close", { sftpId });
export const sftpHome = (sftpId: string) => invoke<string>("sftp_home", { sftpId });
/** Open a remote file in the local default editor; auto-upload on save. */
export const externalEditOpen = (sftpId: string, remotePath: string) =>
  invoke<string>("external_edit_open", { sftpId, remotePath });
export const sftpList = (sftpId: string, path: string) =>
  invoke<FileEntry[]>("sftp_list", { sftpId, path });
export const sftpMkdir = (sftpId: string, path: string) => invoke("sftp_mkdir", { sftpId, path });
export const sftpDelete = (sftpId: string, path: string, isDir: boolean) =>
  invoke("sftp_delete", { sftpId, path, isDir });
export const sftpRename = (sftpId: string, oldPath: string, newPath: string) =>
  invoke("sftp_rename", { sftpId, oldPath, newPath });

export function sftpWatch(onEvent: (ev: TransferEvent) => void): Promise<void> {
  const channel = new Channel<TransferEvent>();
  channel.onmessage = onEvent;
  return invoke("sftp_watch", { onEvent: channel });
}

export const sftpDownload = (sftpId: string, remote: string, local: string, resume = true) =>
  invoke<string>("sftp_download", { sftpId, remote, local, resume });
export const sftpUpload = (sftpId: string, local: string, remote: string, resume = false) =>
  invoke<string>("sftp_upload", { sftpId, local, remote, resume });
export const sftpCancel = (transferId: string) => invoke<boolean>("sftp_cancel", { transferId });

export const scpUpload = (sessionId: string, local: string, remote: string) =>
  invoke("scp_upload", { sessionId, local, remote });
export const scpDownload = (sessionId: string, remote: string, local: string) =>
  invoke("scp_download", { sessionId, remote, local });

// ---------- FTP (plain) ----------

export interface FtpConfig {
  host: string;
  port?: number;
  username?: string;
  password?: string;
}

export const ftpOpen = (config: FtpConfig) => invoke<string>("ftp_open", { config });
export const ftpClose = (ftpId: string) => invoke("ftp_close", { ftpId });
export const ftpList = (ftpId: string, path: string) =>
  invoke<FileEntry[]>("ftp_list", { ftpId, path });
export const ftpMkdir = (ftpId: string, path: string) => invoke("ftp_mkdir", { ftpId, path });
export const ftpDelete = (ftpId: string, path: string, isDir: boolean) =>
  invoke("ftp_delete", { ftpId, path, isDir });
export const ftpRename = (ftpId: string, oldPath: string, newPath: string) =>
  invoke("ftp_rename", { ftpId, oldPath, newPath });
export const ftpDownload = (ftpId: string, remote: string, local: string) =>
  invoke("ftp_download", { ftpId, remote, local });
export const ftpUpload = (ftpId: string, local: string, remote: string) =>
  invoke("ftp_upload", { ftpId, local, remote });

// ---------- local fs ----------

export const fsList = (path: string) => invoke<FileEntry[]>("fs_list", { path });
export const fsHome = () => invoke<string>("fs_home");
export const fsMkdir = (path: string) => invoke("fs_mkdir", { path });
export const fsDelete = (path: string, isDir: boolean) => invoke("fs_delete", { path, isDir });
export const fsRename = (oldPath: string, newPath: string) =>
  invoke("fs_rename", { oldPath, newPath });
export const fsRoots = () => invoke<string[]>("fs_roots");

// ---------- audit logs ----------

export interface LogFileRecord {
  id: string;
  sessionId: string | null;
  path: string;
  startedAt: number;
  endedAt: number | null;
  bytes: number;
}

export interface LogListItem extends LogFileRecord {
  exists: boolean;
}

export const logList = () => invoke<LogListItem[]>("log_list");
export const logDelete = (id: string, deleteFile: boolean) =>
  invoke("log_delete", { id, deleteFile });
export const logTail = (path: string, maxBytes?: number) =>
  invoke<string>("log_tail", { path, maxBytes: maxBytes ?? null });
export const logOpenFolder = (path: string) => invoke("log_open_folder", { path });
