#!/usr/bin/env bash
# OpenTerm — 银河麒麟 V10(Kylin 10)本机构建脚本
#
# 为什么必须在麒麟本机构建:Tauri Linux 版依赖系统 WebKitGTK 运行时,
# 无法从 Windows/macOS 交叉编译出可用产物。在目标麒麟机器上执行本脚本即可。
# 支持 x86_64 与 aarch64(飞腾/鲲鹏),产物自动匹配当前架构。
#
# 用法:
#   chmod +x scripts/build-kylin.sh
#   ./scripts/build-kylin.sh
#
# 产物位置: src-tauri/target/release/bundle/
#   rpm/   *.rpm        —— 麒麟 V10 推荐(rpm 系)
#   appimage/*.AppImage —— 免安装便携(需 FUSE)
#   deb/   *.deb        —— 仅当系统是 Debian 系分支时使用
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> [1/4] 安装系统依赖(webkit2gtk / gtk3 / 工具链)"
if command -v yum >/dev/null 2>&1; then
  sudo yum install -y \
    webkit2gtk3-devel gtk3-devel librsvg2-devel \
    libappindicator-gtk3-devel libsoup-devel \
    openssl-devel perl pkgconfig gcc gcc-c++ make \
    rpm-build libudev-devel || true
elif command -v apt >/dev/null 2>&1; then
  sudo apt update
  sudo apt install -y \
    libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
    libappindicator3-dev libsoup-3.0-dev \
    libssl-dev pkg-config build-essential \
    rpm libudev-dev || true
fi

echo "==> [2/4] 检查 Rust 与 Node"
if ! command -v cargo >/dev/null 2>&1; then
  echo "    安装 rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
if ! command -v node >/dev/null 2>&1; then
  echo "错误: 未找到 Node.js。请安装 Node 18+ (建议用 nvm 或麒麟商店)。" >&2
  exit 1
fi
echo "    rust: $(rustc -V) / node: $(node -v)"

echo "==> [3/4] 安装前端依赖"
npm ci

echo "==> [4/4] 构建(rpm + appimage;Debian 系分支自动追加 deb)"
BUNDLES="rpm,appimage"
if command -v apt >/dev/null 2>&1 && ! command -v yum >/dev/null 2>&1; then
  BUNDLES="deb,rpm,appimage"
fi
npm run tauri build -- --bundles "$BUNDLES"

echo
echo "完成。产物:"
find src-tauri/target/release/bundle -maxdepth 2 -type f \
  \( -name '*.rpm' -o -name '*.deb' -o -name '*.AppImage' \) -print
