<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useAppStore } from "../../stores/app";
import {
  credentialCreate,
  credentialSecret,
  credentialUpdate,
  serialListPorts,
  sessionUpsert,
  sshTestConnect,
  type AuthMethod,
  type JumpHop,
  type ProxyConfig,
  type SerialPortInfo,
  type SessionOptions,
} from "../../ipc/client";

const props = defineProps<{
  modelValue: boolean;
  editId: string | null;
}>();
const emit = defineEmits<{ (e: "update:modelValue", v: boolean): void }>();

const store = useAppStore();

type AuthKind = "password" | "keyboardInteractive" | "key" | "agent";
type Protocol = "ssh" | "telnet" | "rlogin" | "serial" | "local";

const DEFAULT_PORT: Record<Protocol, number> = {
  ssh: 22,
  telnet: 23,
  rlogin: 513,
  serial: 0,
  local: 0,
};

const form = reactive({
  name: "",
  groupId: "" as string,
  protocol: "ssh" as Protocol,
  host: "",
  port: 22,
  username: "",
  authKind: "password" as AuthKind,
  password: "",
  keyPath: "",
  passphrase: "",
  keepaliveSecs: 30,
  reconnect: true,
  term: "xterm-256color",
  x11: false,
  proxyType: "" as "" | "socks5" | "socks4" | "http",
  proxyHost: "",
  proxyPort: 1080,
  // serial
  serialPort: "",
  baudRate: 9600,
  dataBits: 8,
  stopBits: 1,
  parity: "none",
  flowControl: "none",
  // local shell
  shell: "powershell",
  shellCwd: "",
});

const serialPorts = ref<SerialPortInfo[]>([]);

const hops = ref<Array<{ host: string; port: number; username: string; password: string }>>([]);
const saving = ref(false);
const error = ref("");
const testing = ref(false);
const testResult = ref<{ ok: boolean; text: string } | null>(null);

// existing credential id when editing (secret kept if left blank)
let existingCredId: string | null = null;

const visible = computed(() => props.modelValue);

watch(visible, async (v) => {
  if (!v) return;
  error.value = "";
  testResult.value = null;
  existingCredId = null;
  hops.value = [];
  serialPorts.value = await serialListPorts().catch(() => []);
  const rec = props.editId ? store.sessions.find((s) => s.id === props.editId) : null;
  if (rec) {
    form.name = rec.name;
    form.groupId = rec.groupId ?? "";
    form.protocol = (rec.protocol as Protocol) || "ssh";
    form.host = rec.host;
    form.port = rec.port;
    form.username = rec.username;
    existingCredId = rec.credentialId;
    const meta = store.credentials.find((c) => c.id === rec.credentialId);
    form.authKind = (meta?.kind as AuthKind) ?? "agent";
    form.password = "";
    form.keyPath = "";
    form.passphrase = "";
    const opts: SessionOptions = JSON.parse(rec.options || "{}");
    form.keepaliveSecs = opts.keepaliveSecs ?? 30;
    form.reconnect = opts.reconnect ?? true;
    form.term = opts.term ?? "xterm-256color";
    form.x11 = opts.x11 ?? false;
    form.proxyType = (opts.proxy?.type as typeof form.proxyType) ?? "";
    form.proxyHost = opts.proxy?.host ?? "";
    form.proxyPort = opts.proxy?.port ?? 1080;
    hops.value = (opts.jumpChain ?? []).map((h: JumpHop) => ({
      host: h.host,
      port: h.port,
      username: h.username,
      password: "",
    }));
    // protocol-specific restore
    if (rec.protocol === "serial") {
      form.serialPort = rec.host;
      form.baudRate = opts.serial?.baudRate ?? 9600;
      form.dataBits = opts.serial?.dataBits ?? 8;
      form.stopBits = opts.serial?.stopBits ?? 1;
      form.parity = opts.serial?.parity ?? "none";
      form.flowControl = opts.serial?.flowControl ?? "none";
    }
    if (rec.protocol === "local") {
      form.shell = opts.localShell?.shell ?? "powershell";
      form.shellCwd = opts.localShell?.cwd ?? "";
    }
  } else {
    Object.assign(form, {
      name: "",
      groupId: "",
      protocol: "ssh",
      host: "",
      port: 22,
      username: "",
      authKind: "password",
      password: "",
      keyPath: "",
      passphrase: "",
      keepaliveSecs: 30,
      reconnect: true,
      term: "xterm-256color",
      x11: false,
      proxyType: "",
      proxyHost: "",
      proxyPort: 1080,
      serialPort: "",
      baudRate: 9600,
      dataBits: 8,
      stopBits: 1,
      parity: "none",
      flowControl: "none",
      shell: "powershell",
      shellCwd: "",
    });
  }
});

watch(
  () => form.protocol,
  (p) => {
    form.port = DEFAULT_PORT[p] || form.port;
  }
);

function addHop() {
  hops.value.push({ host: "", port: 22, username: "", password: "" });
}
function removeHop(i: number) {
  hops.value.splice(i, 1);
}

function buildOptions(): SessionOptions {
  const jumpChain: JumpHop[] = hops.value
    .filter((h) => h.host && h.username)
    .map((h) => ({
      host: h.host,
      port: h.port,
      username: h.username,
      auth: h.password ? { type: "password", password: h.password } : ({ type: "agent" } as const),
    }));
  const proxy: ProxyConfig | null = form.proxyType
    ? ({ type: form.proxyType, host: form.proxyHost, port: form.proxyPort } as ProxyConfig)
    : null;
  const options: SessionOptions = { term: form.term };
  if (form.protocol === "ssh") {
    options.jumpChain = jumpChain;
    options.proxy = proxy;
    options.keepaliveSecs = form.keepaliveSecs;
    options.reconnect = form.reconnect;
    options.x11 = form.x11;
  } else if (form.protocol === "serial") {
    options.serial = {
      baudRate: form.baudRate,
      dataBits: form.dataBits,
      stopBits: form.stopBits,
      parity: form.parity,
      flowControl: form.flowControl,
    };
  } else if (form.protocol === "local") {
    options.localShell = { shell: form.shell, cwd: form.shellCwd || undefined };
  }
  return options;
}

function effectiveHost(): string {
  if (form.protocol === "serial") return form.serialPort;
  if (form.protocol === "local") return "localhost";
  return form.host;
}

function validate(): string {
  if (!form.name) return "名称必填";
  if (form.protocol === "ssh" && (!form.host || !form.username)) return "SSH 需要主机和用户名";
  if ((form.protocol === "telnet" || form.protocol === "rlogin") && !form.host)
    return "主机必填";
  if (form.protocol === "rlogin" && !form.username) return "Rlogin 需要用户名";
  if (form.protocol === "serial" && !form.serialPort) return "请选择串口";
  return "";
}

async function persistCredential(): Promise<string | null> {
  if (form.protocol !== "ssh" || form.authKind === "agent") return null;
  let credId = existingCredId;
  const secretProvided =
    ((form.authKind === "password" || form.authKind === "keyboardInteractive") &&
      !!form.password) ||
    (form.authKind === "key" && !!(form.keyPath || form.passphrase));
  const secret =
    form.authKind === "key"
      ? { keyPath: form.keyPath, passphrase: form.passphrase || undefined }
      : { password: form.password };
  if (credId && secretProvided) {
    await credentialUpdate(credId, {
      name: form.name,
      kind: form.authKind,
      username: form.username,
      secret,
    });
  } else if (!credId) {
    credId = await credentialCreate({
      name: form.name,
      kind: form.authKind,
      username: form.username,
      secret,
    });
  }
  return credId;
}

async function connectAdhoc() {
  const opts = buildOptions();
  if (form.protocol === "ssh") {
    const auth =
      form.authKind === "agent"
        ? ({ type: "agent" } as const)
        : form.authKind === "key"
          ? ({
              type: "key",
              path: form.keyPath,
              passphrase: form.passphrase || undefined,
            } as const)
          : ({
              type: form.authKind,
              password: form.password,
            } as const);
    await store.openAdhoc(form.name, {
      host: form.host,
      port: form.port,
      username: form.username,
      auth,
      jumpChain: opts.jumpChain ?? [],
      proxy: opts.proxy ?? null,
      keepaliveSecs: opts.keepaliveSecs ?? 30,
      reconnect: opts.reconnect ?? false,
      term: opts.term,
      x11: opts.x11 ?? false,
    });
    return;
  }
  await store.openSession({
    id: "",
    groupId: null,
    sort: 0,
    name: form.name,
    protocol: form.protocol,
    host: effectiveHost(),
    port: form.port,
    username: form.username,
    credentialId: null,
    options: JSON.stringify(opts),
  } as never);
}

/** Build an AuthMethod from the form; when editing and the secret fields are
 *  left blank, fall back to the stored credential (requires unlocked vault). */
async function buildAuthForTest(): Promise<AuthMethod> {
  let { password, keyPath, passphrase } = form;
  const secretBlank =
    (form.authKind === "password" || form.authKind === "keyboardInteractive") && !password;
  const keyBlank = form.authKind === "key" && !keyPath && !passphrase;
  if (existingCredId && (secretBlank || keyBlank) && store.unlocked) {
    const sec = (await credentialSecret(existingCredId)) ?? {};
    if (secretBlank) password = sec.password ?? "";
    if (keyBlank) {
      keyPath = sec.keyPath ?? "";
      passphrase = sec.passphrase ?? "";
    }
  }
  switch (form.authKind) {
    case "agent":
      return { type: "agent" };
    case "key":
      return { type: "key", path: keyPath, passphrase: passphrase || undefined };
    default:
      return { type: form.authKind, password };
  }
}

/** 「测试连接」: connect + authenticate with the current form values. */
async function testConnection() {
  testResult.value = null;
  if (!form.host || !form.username) {
    testResult.value = { ok: false, text: "请先填写主机和用户名" };
    return;
  }
  testing.value = true;
  try {
    const opts = buildOptions();
    const msg = await sshTestConnect({
      host: form.host,
      port: form.port,
      username: form.username,
      auth: await buildAuthForTest(),
      jumpChain: opts.jumpChain ?? [],
      proxy: opts.proxy ?? null,
      keepaliveSecs: opts.keepaliveSecs ?? 30,
      term: opts.term,
    });
    testResult.value = { ok: true, text: msg };
  } catch (e) {
    testResult.value = { ok: false, text: String(e) };
  } finally {
    testing.value = false;
  }
}

async function save(connectAfter: boolean) {
  error.value = validate();
  if (error.value) return;
  if (form.protocol === "ssh" && !store.unlocked && form.authKind !== "agent") {
    error.value = "保险库已锁定,无法保存凭证";
    return;
  }
  saving.value = true;
  try {
    if (connectAfter) {
      // Ad-hoc: nothing is persisted (no session record, no credential).
      await connectAdhoc();
    } else {
      const credId = await persistCredential();
      await sessionUpsert({
        id: props.editId ?? undefined,
        groupId: form.groupId || null,
        name: form.name,
        protocol: form.protocol,
        host: effectiveHost(),
        port: form.port,
        username: form.username,
        credentialId: credId,
        options: JSON.stringify(buildOptions()),
      });
      await store.refreshAll();
    }
    emit("update:modelValue", false);
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}

function close() {
  emit("update:modelValue", false);
}
</script>

<template>
  <div v-if="visible" class="overlay" @click.self="close">
    <div class="dialog">
      <h3>{{ editId ? "编辑会话" : "新建会话" }}</h3>
      <div class="grid">
        <label>名称</label>
        <input v-model="form.name" placeholder="生产服务器" />

        <label>分组</label>
        <select v-model="form.groupId">
          <option value="">未分组</option>
          <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
        </select>

        <label>协议</label>
        <select v-model="form.protocol">
          <option value="ssh">SSH</option>
          <option value="telnet">Telnet(明文)</option>
          <option value="rlogin">Rlogin(明文)</option>
          <option value="serial">Serial 串口</option>
          <option value="local">本地 Shell</option>
        </select>

        <template v-if="form.protocol !== 'serial' && form.protocol !== 'local'">
          <label>主机</label>
          <div class="row">
            <input v-model="form.host" placeholder="192.168.1.10" style="flex: 1" />
            <input v-model.number="form.port" type="number" style="width: 90px" />
          </div>
        </template>

        <label v-if="form.protocol === 'ssh' || form.protocol === 'rlogin'">用户名</label>
        <input
          v-if="form.protocol === 'ssh' || form.protocol === 'rlogin'"
          v-model="form.username"
          placeholder="root"
        />

        <div v-if="form.protocol === 'telnet' || form.protocol === 'rlogin'" class="warn">
          明文协议:凭据与数据不加密,仅在可信网络使用。
        </div>
      </div>

      <!-- ===== SSH ===== -->
      <div v-if="form.protocol === 'ssh'" class="grid">
        <label>认证</label>
        <select v-model="form.authKind">
          <option value="password">密码</option>
          <option value="keyboardInteractive">键盘交互</option>
          <option value="key">公钥</option>
          <option value="agent">Agent</option>
        </select>

        <template v-if="form.authKind === 'password' || form.authKind === 'keyboardInteractive'">
          <label>密码</label>
          <input
            v-model="form.password"
            type="password"
            :placeholder="existingCredId ? '留空则保持不变' : ''"
          />
        </template>

        <template v-if="form.authKind === 'key'">
          <label>私钥路径</label>
          <input v-model="form.keyPath" placeholder="C:\Users\x\.ssh\id_ed25519 或 .ppk" />
          <label>私钥口令</label>
          <input v-model="form.passphrase" type="password" placeholder="可空" />
        </template>

        <label>跳板机</label>
        <div class="hops">
          <div v-for="(h, i) in hops" :key="i" class="hop-row">
            <input v-model="h.host" placeholder="host" />
            <input v-model.number="h.port" type="number" class="p" />
            <input v-model="h.username" placeholder="user" />
            <input v-model="h.password" type="password" placeholder="password(空=agent)" />
            <a class="rm" @click="removeHop(i)">✕</a>
          </div>
          <a class="add" @click="addHop">＋ 添加跳板</a>
        </div>

        <label>代理</label>
        <div class="row">
          <select v-model="form.proxyType" style="width: 110px">
            <option value="">无</option>
            <option value="socks5">SOCKS5</option>
            <option value="socks4">SOCKS4</option>
            <option value="http">HTTP</option>
          </select>
          <input v-model="form.proxyHost" placeholder="127.0.0.1" style="flex: 1" :disabled="!form.proxyType" />
          <input v-model.number="form.proxyPort" type="number" style="width: 90px" :disabled="!form.proxyType" />
        </div>

        <label>保活(秒)</label>
        <input v-model.number="form.keepaliveSecs" type="number" />

        <label>断线重连</label>
        <input v-model="form.reconnect" type="checkbox" class="cb" />

        <label>X11 转发</label>
        <div class="row">
          <input v-model="form.x11" type="checkbox" class="cb" style="width: auto" />
          <span class="warn" style="grid-column: auto">需本地 X Server(Windows: VcXsrv/Xming;macOS: XQuartz)</span>
        </div>
      </div>

      <!-- ===== Serial ===== -->
      <div v-else-if="form.protocol === 'serial'" class="grid">
        <label>串口</label>
        <select v-model="form.serialPort">
          <option value="" disabled>选择串口</option>
          <option v-for="p in serialPorts" :key="p.name" :value="p.name">
            {{ p.name }} — {{ p.description }}
          </option>
          <option v-if="form.serialPort" :value="form.serialPort">{{ form.serialPort }}</option>
        </select>

        <label>波特率</label>
        <select v-model.number="form.baudRate">
          <option v-for="b in [110, 300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600]" :key="b" :value="b">{{ b }}</option>
        </select>

        <label>数据位</label>
        <select v-model.number="form.dataBits">
          <option :value="5">5</option>
          <option :value="6">6</option>
          <option :value="7">7</option>
          <option :value="8">8</option>
        </select>

        <label>停止位</label>
        <select v-model.number="form.stopBits">
          <option :value="1">1</option>
          <option :value="2">2</option>
        </select>

        <label>校验</label>
        <select v-model="form.parity">
          <option value="none">None</option>
          <option value="odd">Odd</option>
          <option value="even">Even</option>
        </select>

        <label>流控</label>
        <select v-model="form.flowControl">
          <option value="none">None</option>
          <option value="hardware">RTS/CTS</option>
          <option value="software">XON/XOFF</option>
        </select>
      </div>

      <!-- ===== Local shell ===== -->
      <div v-else-if="form.protocol === 'local'" class="grid">
        <label>Shell</label>
        <select v-model="form.shell">
          <option value="powershell">PowerShell</option>
          <option value="cmd">CMD</option>
          <option value="pwsh">PowerShell 7 (pwsh)</option>
          <option value="wsl">WSL</option>
          <option value="bash">bash</option>
        </select>

        <label>工作目录</label>
        <input v-model="form.shellCwd" placeholder="可空,默认用户目录" />
      </div>

      <div v-if="error" class="err">{{ error }}</div>
      <div v-if="testResult" class="test-result" :class="{ ok: testResult.ok }">
        {{ testResult.text }}
      </div>
      <div class="btns">
        <button
          v-if="form.protocol === 'ssh'"
          class="ghost"
          :disabled="testing || saving"
          @click="testConnection"
        >
          {{ testing ? "测试中…" : "测试连接" }}
        </button>
        <button class="ghost" @click="close">取消</button>
        <button class="ghost" :disabled="saving" @click="save(true)">仅连接</button>
        <button class="primary" :disabled="saving" @click="save(false)">
          {{ saving ? "保存中…" : "保存" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.dialog {
  width: 520px;
  max-height: 84vh;
  overflow-y: auto;
  background: #2b2b2c;
  border: 1px solid #4a4a4a;
  border-radius: 8px;
  padding: 18px 20px;
}
h3 {
  margin: 0 0 14px;
  font-size: 15px;
}
.grid {
  display: grid;
  grid-template-columns: 84px 1fr;
  gap: 8px 10px;
  align-items: center;
  font-size: 12.5px;
}
label {
  color: #aaa;
  text-align: right;
}
input,
select {
  background: #1e1e1f;
  border: 1px solid #4a4a4a;
  color: #ddd;
  padding: 5px 8px;
  border-radius: 4px;
  font-size: 12.5px;
}
input:focus,
select:focus {
  outline: 1px solid #0e639c;
}
.cb {
  justify-self: start;
}
.row {
  display: flex;
  gap: 6px;
}
.hops {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.hop-row {
  display: flex;
  gap: 4px;
  align-items: center;
}
.hop-row input {
  width: 0;
  flex: 1;
}
.hop-row .p {
  flex: 0 0 56px;
}
.rm {
  color: #999;
  cursor: pointer;
}
.add {
  color: #4fc3f7;
  cursor: pointer;
  font-size: 12px;
}
.err {
  color: #f77;
  font-size: 12px;
  margin-top: 10px;
}
.test-result {
  margin-top: 10px;
  font-size: 12px;
  color: #f77;
  background: rgba(255, 80, 80, 0.08);
  border: 1px solid rgba(255, 80, 80, 0.3);
  border-radius: 4px;
  padding: 6px 10px;
  word-break: break-all;
}
.test-result.ok {
  color: #7ee787;
  background: rgba(80, 255, 140, 0.08);
  border-color: rgba(80, 255, 140, 0.3);
}
.warn {
  grid-column: 2;
  color: #d19a66;
  font-size: 12px;
}
.btns {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.primary {
  background: #0e639c;
  color: #fff;
  border: none;
  padding: 6px 18px;
  border-radius: 4px;
  cursor: pointer;
}
.ghost {
  background: transparent;
  color: #aaa;
  border: 1px solid #555;
  padding: 6px 18px;
  border-radius: 4px;
  cursor: pointer;
}
</style>
