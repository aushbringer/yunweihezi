# OpenTerm 系统架构与设计文档

> 跨平台运维终端(对标 Xshell / FinalShell / MobaXterm)
> 技术栈:**Tauri v2 + Rust(tokio)+ Vue 3 + TypeScript + Pinia + xterm.js 6**

---

## 1. 技术语言与选型

| 层 | 语言 / 框架 | 选型理由 |
|---|---|---|
| 桌面壳 | **Tauri v2**(Rust) | 安装包 4–6 MB(Electron 的 1/20),内存占用低,原生 WebView(Win: WebView2 / Linux: WebKitGTK),适合信创环境 |
| 后端核心 | **Rust + tokio** | 内存安全、零 GC 停顿;SSH/SFTP/串口/Telnet 全是长连接 IO,tokio 异步天然契合 |
| SSH 协议栈 | **russh 0.65 / russh-sftp 3.0** | 纯 Rust SSH2 实现,无 C 依赖,审计友好 |
| 前端 | **Vue 3 + TypeScript + Pinia** | 组件化 + 类型安全;组合式 API 便于逻辑复用 |
| 终端渲染 | **xterm.js 6 + WebGL addon** | 业界标准终端模拟器(VSCode 同款),WebGL 加速滚动 |
| 持久化 | **SQLite(rusqlite 同步池化)** | 会话配置/触发器/宏/日志元数据,零外部服务 |
| 加密 | **AES-256-GCM + scrypt** | 会话密码加密落库,主密钥派生自用户口令 |

---

## 2. 总体架构

```
┌───────────────────────────── Tauri 主窗口 / 分离窗口(detached-*) ─────────────────────────────┐
│  Vue 3 前端 (src/)                                                                             │
│  ├─ views/       MainView(主界面) · DetachedView(独立终端窗口)                                │
│  ├─ components/  TerminalView(xterm 封装) · TerminalTabs(标签/广播/分离)                      │
│  │               SftpPanel(双面板文件管理) · SessionDialog · TerminalSettingsDialog …         │
│  ├─ stores/      app(标签/会话状态) · transfers(传输队列) · terminalSettings · macros …       │
│  └─ ipc/         client.ts(invoke/Channel 封装) · notify.ts(系统通知)                        │
└──────────────┬───────────────────────────────────────▲────────────────────────────────────────┘
               │ invoke(命令, JSON)                    │ Channel<OutputEvent>(字节流, base64)
┌──────────────▼───────────────────────────────────────┴────────────────────────────────────────┐
│  Rust 后端 (src-tauri/src/)                                                                    │
│  ├─ commands/    Tauri 命令层(参数校验 → core 调度)                                           │
│  ├─ core/                                                                                     │
│  │   ├─ session/  SessionManager:五协议统一生命周期(ssh/local/telnet/rlogin/serial)          │
│  │   ├─ terminal/ SessionPipe(可换 Channel + taps 侧链) · TriggerEngine(正则扫描)            │
│  │   ├─ ssh/      russh 客户端 · shell 通道 · 本地/远程/动态转发 · X11 桥接                    │
│  │   ├─ sftp/     双面板后端 · 传输队列(进度事件) · 外部编辑器监听回传                          │
│  │   ├─ serial/   串口 IO 线程 · X/Y/ZModem                                                   │
│  │   ├─ crypto/   AES-256-GCM 保险库(口令 → scrypt → KEK)                                   │
│  │   └─ db.rs     SQLite(会话/分组/触发器/宏/工具栏/日志索引)                                  │
│  └─ state.rs     AppState(db 句柄 + 数据目录)                                                 │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**设计主线:一条双向数据管道。**
键盘输入 → `invoke session_input` → `SessionManager` → 协议任务;
协议输出 → `SessionPipe` → `Channel<OutputEvent>` → xterm.js。
所有协议(SSH/串口/Telnet/Rlogin/本地 PTY)都收敛到这一对原语,前端零感知差异。

---

## 3. 核心设计思路

### 3.1 SessionPipe:可热插拔的输出管道 + 侧链 tap

终端输出不止给屏幕——审计日志、触发器引擎也要同一份字节流。设计为 **主 Channel + 任意多个只读 tap**,避免三处各自解析转义序列:

```rust
// src-tauri/src/core/terminal/pipe.rs
#[derive(Clone)]
pub struct SessionPipe {
    // Arc<Mutex<Channel>>:分离窗口通过 session_attach 换入新 Channel,
    // 输出流无缝切换窗口,会话本身不断线
    channel: std::sync::Arc<std::sync::Mutex<Channel<OutputEvent>>>,
    // 审计日志、触发器引擎作为 tap 挂在旁路,零成本共享原始字节
    taps: Vec<std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>>,
}

pub fn push_data(&self, bytes: &[u8]) {
    for tap in &self.taps { tap(bytes); }           // 日志落盘 + 触发器扫描
    self.send(OutputEvent::Data { data: b64(bytes) }); // 推送前端
}
```

### 3.2 SessionManager:五协议一个抽象

每种协议一个 tokio 任务,统一收 `SessionCommand`(Input/Resize/Close),统一推 `SessionPipe`。会话注册表持有 pipe/ssh_handle/serial 控制柄,供 SFTP、隧道、分离窗口复用:

```rust
// src-tauri/src/core/session/manager.rs
pub async fn open_ssh(&self, cfg: SshSessionConfig, pipe: SessionPipe) -> AppResult<String> {
    let conn = client::connect(&cfg).await?;                       // 跳板链/代理/认证
    let channel = shell::open_shell(&conn.handle, &cfg.term,
                                    cfg.cols, cfg.rows, cfg.x11).await?; // PTY + X11
    self.sessions.lock().await.insert(id.clone(), SessionEntry {
        cmd_tx, kind: "ssh", pipe: pipe.clone(),
        ssh_handle: Some(conn.handle.clone()),  // SFTP/隧道共享同一连接
        router: Some(conn.router.clone()), serial: None,
    });
    tokio::spawn(ssh_session_task(...));  // 读写循环 + 断线指数退避重连
    Ok(id)
}
```

### 3.3 前端:xterm.js 封装与窗口分离

`TerminalView` 是唯一碰 xterm 的组件:加载 Fit/Search/WebGL/Serialize 四个 addon,`handleOutput` 喂字节,`onData` 回传输入(广播感知)。**分离窗口**利用 SerializeAddon 把当前屏幕+回滚序列化为转义序列,经 localStorage 交接给新窗口,新窗口 `session_attach` 换入 Channel 后原样回放——用户看到的是"标签页变成了独立窗口,内容一行不少":

```ts
// src/components/terminal/TerminalTabs.vue
async function detachTab(tab: Tab) {
  const serialized = termRefs.value[tab.id]?.serialize() ?? "";  // 打包屏幕
  localStorage.setItem(`detach:${tab.sessionId}`,
    JSON.stringify({ title: tab.title, serialized }));
  new WebviewWindow(`detached-${tab.sessionId}`, {
    url: `index.html?detach=${tab.sessionId}` });                // 开新窗口
  store.detachTabLocal(tab.id);   // 本地移除标签,但不断后端会话
}
```

### 3.4 触发器引擎:旁路扫描,四种动作分流

输出字节流经 tap 进入 `TriggerEngine`(编译后的正则集合 + 行缓冲),命中后按动作分流——`send`/`script` 后端直接执行,`highlight`/`alert` 推前端:

```rust
// src-tauri/src/commands/session_cmds.rs
match fire.action.as_str() {
    "send" => { /* 模板展开 {match} 后写回会话,自动补回车 */ }
    "script" => { /* spawn_blocking 跑本地脚本 */ }
    _ => pipe.push_trigger(...),  // 前端高亮 / toast + 系统通知
}
```

### 3.5 安全设计

- **配置加密**:密码/私钥口令 → AES-256-GCM;主密钥 = scrypt(用户口令),不落盘
- **能力最小化**:Tauri capabilities 白名单,`windows: ["main", "detached-*"]` 精确到窗口标签
- **X11 cookie**:每次连接 uuid 随机 32-hex,由 sshd 自行 xauth 登记
- **审计**:会话原始字节流 tap 直接落盘,含时间戳与会话元数据

### 3.6 外部编辑器:下载-监听-回传闭环

SFTP 远端文件右键「外部编辑」→ 下载到 `%TEMP%/openterm-edit/` → 系统默认程序打开 → 后台任务每 1.5s 比对 (len, mtime),**变化且连续两次采样稳定**(编辑器写入完成)即自动上传回远端:

```rust
// src-tauri/src/core/external_editor.rs
if pending == Some(cur) {                       // 第二次采到同样的新指纹 = 写完了
    let bytes = tokio::fs::read(&local).await?;
    sftp.write(&remote, &bytes).await?;         // 回传
    baseline = Some(cur);
} else { pending = Some(cur); }                 // 第一次发现变化,再观察一轮
```

---

## 4. 端到端示例:一次 SSH 会话的建立

**前端发起**(stores/app.ts):

```ts
const sessionId = await sessionOpen({
  host: record.host, port: record.port, username: record.username,
  auth: { type: "password", password },
  keepaliveSecs: 30, reconnect: true, x11: false,
}, (ev) => outputHandler(tab.id, ev));   // Channel 回调注册输出处理
```

**后端装配管道**(commands/session_cmds.rs):

```rust
let pipe = SessionPipe::new(on_output);          // 1. 建管道
let pipe = with_logging(&state, pipe, vars).await;   // 2. 挂审计 tap
let (pipe, rx) = with_triggers(&state, pipe, record_id).await; // 3. 挂触发器 tap
let sid = sessions.open_ssh(config, pipe.clone()).await?;      // 4. 开会话
spawn_trigger_dispatch(sessions, sid, pipe, rx);               // 5. 动作分发
```

**xterm 接收**(components/TerminalView.vue):

```ts
function handleOutput(ev: OutputEvent) {
  if (ev.kind === "data") term?.write(b64decode(ev.data)); // 字节 → 屏幕
  else if (ev.kind === "state") emit("state", ev.state, ev.message);
}
```

---

## 5. 跨平台构建

| 平台 | 产物 | 方式 |
|---|---|---|
| Windows 10/11 | `msi` / `nsis setup.exe`(已产出,4.6–6.4 MB) | `npm run tauri build` |
| 银河麒麟 V10 | `rpm` / `AppImage` | 本机跑 `scripts/build-kylin.sh`(WebKitGTK 运行时依赖,无法交叉编译;x86_64/aarch64 自动匹配) |
| macOS / 通用 Linux | `dmg` / `deb` | 同源码各平台原生构建 |

**已知限制**(见实施计划 P3):SSH1 不支持(russh 仅 SSH2);RDP 二期评估 FreeRDP;PKCS#11 二期,当前可经 Pageant/ssh-agent 迂回。Python 脚本引擎以 cargo feature 默认关闭,运行时需要 python3.dll ≥ 3.9。

---

## 6. 目录速查

```
e:\CodeProject\1xuexitong
├─ src/                    # Vue3 前端
│  ├─ views/               # MainView · DetachedView
│  ├─ components/terminal/ # TerminalView · TerminalTabs · ComposePane · HexViewer
│  ├─ components/sftp/     # SftpPanel(双面板)
│  ├─ stores/              # Pinia:app/transfers/terminalSettings/macros/toolbar
│  └─ ipc/                 # client.ts(全部 invoke/Channel 封装)· notify.ts
├─ src-tauri/src/
│  ├─ commands/            # 命令层(前端可调的 #[tauri::command])
│  ├─ core/                # session/terminal/ssh/sftp/serial/crypto/logging/db …
│  ├─ capabilities/        # 权限白名单
│  └─ tauri.conf.json      # 窗口(透明)/打包配置
├─ scripts/build-kylin.sh  # 麒麟 V10 一键构建
└─ .trae/documents/ops-terminal-implementation-plan.md  # 需求→实现对照
```
