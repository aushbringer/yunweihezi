# 构建指南

适用于 v1.0.13。产物统一输出到 `src-tauri/target/release/bundle/`。

---

## 1. 通用环境

| 工具 | 版本要求 | 说明 |
|---|---|---|
| Node.js | ≥ 18（建议 20 LTS） | 前端构建 |
| npm | 随 Node | 依赖管理 |
| Rust | ≥ 1.77 stable | 后端编译，通过 [rustup](https://rustup.rs) 安装 |
| Git | 任意 | 拉取源码 |

安装依赖与开发调试：

```bash
npm install
npm run tauri dev
```

---

## 2. Windows

### 2.1 前置组件

1. **Visual Studio 2022 生成工具**：勾选「使用 C++ 的桌面开发」（提供 MSVC 链接器）
2. **WebView2 运行时**：Windows 11 自带；Win10 可从微软官网安装 Evergreen 运行时
3. （可选）**NASM**：部分依赖需要，加入 PATH 即可

> 若 `cargo build` 报 `link.exe not found`，说明 MSVC 工具链未装或未在 PATH 中，请在「x64 Native Tools 命令提示符」下执行，或重装 Build Tools。

### 2.2 构建产物

```powershell
npm run tauri build
```

默认按 `src-tauri/tauri.conf.json` 的 `bundle.targets` 产出：

```
src-tauri/target/release/bundle/
├── nsis/openterm_1.0.13_x64-setup.exe      # 安装包（推荐分发）
├── msi/openterm_1.0.13_x64_en-US.msi       # 企业批量部署
└── portable/openterm_1.0.13_x64-portable.zip  # 免安装
```

只构建某一种：

```powershell
npm run tauri build -- --bundles nsis
npm run tauri build -- --bundles portable
```

### 2.3 NSIS 与 MSI 的区别

| | NSIS（setup.exe） | MSI |
|---|---|---|
| 形态 | 可执行安装向导 | Windows Installer 标准包 |
| 权限 | 默认当前用户，不强制管理员 | 系统级，需要管理员 |
| 适用 | 个人使用、发给同事 | 域环境 GPO / SCCM / Intune 分发 |
| 静默 | `setup.exe /S` | `msiexec /i xxx.msi /qn` |
| 修复 | 重跑安装包 | 控制面板「修复/更改」 |

---

## 3. Linux（麒麟 V10 / 统信 UOS / Ubuntu）

Tauri Linux 版依赖系统 WebKitGTK，**必须在目标 Linux 机器上构建**，无法从 Windows 交叉编译。

### 3.1 依赖

Debian 系：

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev libsoup-3.0-dev libssl-dev \
  build-essential pkg-config libudev-dev
```

RPM 系（银河麒麟 V10）：

```bash
sudo yum install -y webkit2gtk3-devel gtk3-devel librsvg2-devel \
  libappindicator-gtk3-devel libsoup-devel openssl-devel \
  perl pkgconfig gcc gcc-c++ make rpm-build libudev-devel
```

### 3.2 一键构建

仓库内置脚本，自动识别发行版与 CPU 架构（x86_64 / aarch64）：

```bash
chmod +x scripts/build-kylin.sh
./scripts/build-kylin.sh
```

产物：

- `bundle/rpm/*.rpm` —— 麒麟 V10 推荐
- `bundle/appimage/*.AppImage` —— 免安装便携（运行需 FUSE）
- `bundle/deb/*.deb` —— Debian 系分支自动追加

AppImage 运行：

```bash
chmod +x openterm_1.0.13_amd64.AppImage
./openterm_1.0.13_amd64.AppImage
```

> 若麒麟系统缺少 FUSE，可先 `sudo yum install -y fuse fuse-libs`，或使用 `--appimage-extract` 解包运行。

---

## 4. macOS

需 Xcode Command Line Tools：

```bash
xcode-select --install
npm run tauri build
```

产物为 `.app` 与 `.dmg`。未签名应用首次打开需在「系统设置 → 隐私与安全性」中允许。

---

## 5. 可选能力

| 能力 | 开启方式 | 说明 |
|---|---|---|
| Python 脚本引擎 | `npm run tauri build -- --features python` | 需运行环境有 `python3.dll` / `libpython3` ≥ 3.9（abi3） |
| 调试日志 | `RUST_LOG=openterm=debug` | 环境变量控制 tracing 级别 |

---

## 6. 常见构建问题

| 现象 | 原因与处理 |
|---|---|
| `link.exe not found` (Windows) | 未安装 VS Build Tools 的 C++ 桌面开发组件 |
| `webkit2gtk not found` (Linux) | 缺少 dev 包，或版本为 4.0 而 Tauri v2 需要 4.1 |
| `error: failed to run custom build command for libudev-sys` | 安装 `libudev-dev` |
| 构建极慢 / 内存溢出 | `Cargo.toml` 已启用 LTO，属正常；可临时改 `lto = false` 调试 |
| 安装包体积过大 | 检查 `[profile.release]` 是否保留 `strip = true` |
| 麒麟无法启动 | 检查 WebKitGTK 版本与显卡驱动，尝试 `WEBKIT_DISABLE_COMPOSITING_MODE=1` |

---

## 7. CI 自动构建

仓库提供 `.github/workflows/build.yml`：

- push 到 `main` / 提 PR：执行类型检查与构建验证
- 推送 `v*` 标签：自动构建 Windows + Linux 产物并创建 Draft Release

使用前请在仓库 Settings → Actions 中确认工作流权限为可写。
