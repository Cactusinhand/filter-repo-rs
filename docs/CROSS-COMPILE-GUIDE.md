# filter-repo-rs 多平台交叉编译指南

本文档介绍如何为 filter-repo-rs 构建适用于不同操作系统与架构的二进制文件。

## 支持范围

- Linux: x86_64, ARM64（glibc 与 musl 变体）
- macOS: x86_64（Intel）、aarch64（Apple Silicon）
- Windows: x86_64、ARM64（MSVC）

## 环境准备

1) Rust 工具链（stable）

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

2) cross 工具（多数 Linux 交叉编译使用）

```sh
cargo install cross --git https://github.com/cross-rs/cross
```

3) 可选：Docker（cross 默认使用）

## 快速构建

首选使用脚本：

```sh
# Linux/macOS
./scripts/build-cross.sh

# Windows (cmd)
scripts\build-cross.bat
```

或直接使用 cross/cargo：

```sh
# Linux targets
cross build --target x86_64-unknown-linux-gnu --release -p filter-repo-rs
cross build --target aarch64-unknown-linux-gnu --release -p filter-repo-rs

# macOS targets（需在 macOS 上构建）
cargo build --target x86_64-apple-darwin --release -p filter-repo-rs
cargo build --target aarch64-apple-darwin --release -p filter-repo-rs

# Windows targets
cross build --target x86_64-pc-windows-msvc --release -p filter-repo-rs
cross build --target aarch64-pc-windows-msvc --release -p filter-repo-rs
```

## 产物与验证

- 产物重命名复制到 `target/releases/`
- 使用脚本验证：

```sh
./scripts/verify-build.sh
```

该脚本会检查：
- 文件存在与大小
- 非 Windows 目标的执行权限
- 当前平台可运行目标的 `--help` 冒烟测试

## Cargo 配置（摘录）

`.cargo/config.toml` 中包含常用链接器与优化项，例如：

```toml
[profile.release]
lto = true
codegen-units = 1
panic = "abort"
strip = true

[env]
RUSTFLAGS_x86_64_pc_windows_msvc = "-C target-feature=+crt-static"
RUSTFLAGS_aarch64_pc_windows_msvc = "-C target-feature=+crt-static"
```

## 常见问题

1) cross 安装失败：

```sh
cargo install cross --git https://github.com/cross-rs/cross
```

2) Docker 权限问题（Linux）：

```sh
sudo usermod -aG docker $USER
newgrp docker
```

3) macOS 交叉编译：建议在 macOS 本机构建，并安装 Xcode CLT：

```sh
xcode-select --install
```

