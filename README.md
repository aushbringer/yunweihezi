# OpenTerm

> 跨平台运维终端 —— SSH / SFTP / 串口 / Telnet 一站式工作台
> Tauri v2 + Rust(tokio) + Vue 3 + TypeScript + xterm.js

OpenTerm 是一个面向运维场景的轻量级终端工具，对标 Xshell / FinalShell / MobaXterm。
安装包仅 4–6 MB，内存占用低，原生 WebView 渲染，同时适配 Windows 与国产 Linux（麒麟等）。

---

## 功能特性

### 连接协议
- **SSH2**：密码、键盘交互、公钥（OpenSSH / PuTTY PPK）、SSH Agent 四种认证
- **跳板机链**：支持多级跳板，逐跳认证
- **代理**：SOCKS5 / SOCKS4 / HTTP 出站代理
- **Telnet / Rlogin**：兼容老式网络设备（明文，会提示安全风险）
- **串口**：波特率/数据位/停止位/校验/流控可配，内置 XModem / YModem / ZModem 传输
- **本地 Shell**：PowerShell / pwsh / cmd / WSL / bash
- **X11 转发**：需本地 X Server（Windows: VcXsrv / Xming，macOS: XQuartz）

### 终端
- xterm.js 6 + WebGL 渲染，海量滚动不卡顿
- 多标签页、**分离为独立窗口**
- **广播输入**：一次键入同步发送到多个会话（可单独排除某个标签）
- **撰写窗格（Compose）**：多行命令编辑后一次性发送
- 十六进制查看器、字体/主题/透明度/全屏、搜索高亮
- 自定义工具栏布局

### 文件传输
- **SFTP 双面板**：本地 ↔ 远程，拖拽/队列/进度/断点续传/取消
- **SCP** 上传下载
- **FTP**（明文）独立入口
- 远程文件**外部编辑器**打开，保存后自动回传（单文件 32 MB 上限）

### 效率与自动化
- **会话树**：分组管理，会话可**鼠标拖拽**到任意分组
- **测试连接**：创建会话时用当前表单参数真实连接验证
- **触发器**：正则匹配输出，支持高亮 / 告警 / 自动发送 / 执行脚本
- **快速命令栏**：常用命令一键发送
- **键盘宏**：录制回放 + 全局快捷键映射
- **端口转发**：本地(L) / 远程(R) / 动态(SOCKS5)，可保存并随会话自动启动

### 安全
- 会话凭证加密落库：**AES-256-GCM**，主密钥由 **Argon2id** 从口令派生
- **免密码模式**：默认开箱即用，首次启动生成本机随机密钥（`vault.key`）作为加密密钥，无需设置口令
- 可选设置主密码；设置时会自动把已有凭证重加密到新密钥下
- 会话日志审计（写入磁盘 + 元数据索引）
- 配置加密导入导出

---

## 下载安装

前往 [Releases](../../releases) 下载最新版本 **v1.0.13**：

| 文件 | 适用场景 |
|---|---|
| `openterm_1.0.13_x64-setup.exe` | Windows 普通用户，双击安装（NSIS，不强制管理员） |
| `openterm_1.0.13_x64_en-US.msi` | 企业域环境批量部署（GPO / SCCM / Intune） |
| `openterm_1.0.13_x64-portable.zip` | 免安装，解压即用，可放 U 盘 |
| `openterm_1.0.13_amd64.deb` / `.AppImage` | 麒麟 V10 / 统信 UOS / Ubuntu / Debian |

> 系统要求：Windows 10 1809+（需 WebView2 运行时）、Linux 需 WebKitGTK 4.1。
> 麒麟 V10 构建方法见 [docs/BUILD.md](docs/BUILD.md)。

---

## 快速开始

1. 安装并启动 OpenTerm（首次启动无需设置密码，直接进入主界面）
2. 点击左侧「**＋会话**」新建连接，填写主机 / 端口 / 用户名 / 认证方式
3. 点击「**测试连接**」验证配置是否可用
4. 保存后双击会话即可连接；也可「**仅连接**」不保存
5. 需要用密码保护凭证时，点工具栏「**锁定**」并设置主密码

详细操作见 [docs/USER_GUIDE.md](docs/USER_GUIDE.md)。

---

## 从源码构建

### 环境要求
- Node.js ≥ 18、npm
- Rust ≥ 1.77（stable）
- Windows：MSVC 工具链 + WebView2（Win11 自带）
- Linux：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libssl-dev`、`libudev-dev`、`libayatana-appindicator3-dev`

```bash
npm install
npm run tauri dev      # 开发调试
npm run tauri build    # 产出安装包，位于 src-tauri/target/release/bundle/
```

完整构建说明（含麒麟 V10、交叉编译、CI）见 [docs/BUILD.md](docs/BUILD.md)。

---

## 项目结构

```
OpenTerm/
├── src/                      # Vue 3 前端
│   ├── views/                # MainView（主界面）/ DetachedView（分离窗口）
│   ├── components/           # 终端、会话树、SFTP、隧道、宏、触发器…
│   ├── stores/               # Pinia：标签/传输/宏/工具栏/终端设置
│   └── ipc/                  # invoke / Channel 封装
├── src-tauri/                # Rust 后端
│   ├── src/commands/         # Tauri 命令层
│   ├── src/core/             # session / ssh / sftp / serial / crypto / db …
│   └── capabilities/         # Tauri 权限配置
├── docs/                     # 架构、使用手册、构建、FAQ
└── scripts/                  # 构建脚本（麒麟打包等）
```

架构设计详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

---

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面壳 | Tauri v2 |
| 后端 | Rust + tokio（russh 0.65 / russh-sftp 3.0） |
| 前端 | Vue 3 + TypeScript + Pinia + Vite |
| 终端 | xterm.js 6 + WebGL addon |
| 存储 | SQLite（rusqlite，bundled） |
| 加密 | AES-256-GCM + Argon2id |

---

## 已知限制

- 仅支持 SSH2，**不支持 SSH1**
- 不支持 RDP / VNC 图形远程桌面、PKCS#11 硬件令牌
- X11 转发需要本机安装 X Server
- Python 脚本引擎为可选 feature（默认关闭，需 Python ≥ 3.9 运行库）
- Telnet / Rlogin / FTP 为明文协议，仅建议在可信网络使用

更多见 [docs/FAQ.md](docs/FAQ.md)。

---

## 参与贡献

欢迎提 Issue / PR，请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证

[MIT](LICENSE)
