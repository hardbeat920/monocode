
<p align="center">
  <img src="public/monocode.png" alt="MonoCode" width="88" />
</p>

<h1 align="center">MonoCode</h1>

<p align="center">
  <strong>你的编程智能体的桌面端 UI。</strong>
</p>

<p align="center">
  <img width="1680" height="1050" alt="Screenshot 2026-09-04 at 06 34 00" src="https://github.com/user-attachments/assets/2cd4a6ec-eb1e-4b45-8627-a76442ea3874" />
</p>

支持你在 Claude Code、Codex、Cursor、Grok Build、OpenCode、Pi、omp 和 fx 上的订阅。只要它们已安装并登录，MonoCode 就能运行它们。标签页即会话，编辑器即输入。MonoCode 不售卖 Token。

## 安装

> 请先安装并登录至少一个提供商：
>
> - [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
> - [Codex](https://developers.openai.com/codex/cli) - `codex login`
> - [Cursor CLI](https://cursor.com/cli) - `agent login`
> - [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` 然后 `grok login`
> - [OpenCode](https://opencode.ai) - `opencode auth login`
> - [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
> - [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
> - [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` 然后 `fx login`

macOS (Apple Silicon)：下载 [MonoCode.dmg](https://dl.usemono.dev/MonoCode.dmg)，打开并将 MonoCode 拖入"应用程序"文件夹。

Linux (x86_64)：从 [GitHub Releases](https://github.com/hardbeat920/monocode/releases/latest) 下载 `.deb` 或 AppImage。使用 `sudo apt install ./MonoCode_*.deb` 安装 `.deb`，或使用 `chmod +x MonoCode_*.AppImage` 将 AppImage 设为可执行文件后直接运行。

## 一些说明

该项目目前处于非常早期的阶段，你可能会遇到一些 Bug。

欢迎提交小而精的 Pull Request。任何较大的改动请先创建 Issue 进行讨论 — 详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 从源码构建

支持 macOS 和 Linux。

需要 Node.js 20+ 及当前稳定版 Rust 工具链。在 Linux 上，请确保已安装标准的 Tauri 前置依赖（例如 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libsoup-3.0-dev`、`libjavascriptcoregtk-4.1-dev`）。

```bash
npm install
npm run tauri dev
```

### Ubuntu / Debian 安装包

在 Ubuntu/Debian 工作站上，仓库可以自动安装原生 Tauri 前置依赖，并直接构建可分发的 Linux 安装包：

```bash
npm run setup:linux:deb
npm ci
npm run build:linux
```

Linux 构建会在 `target/release/bundle/` 目录下生成 `.deb` 和 AppImage 包。
Tauri 在 Linux 开发和构建时会自动加载 `src-tauri/tauri.linux.conf.json`。

## 许可证

[MIT](LICENSE)。提供商名称和商标归其各自所有者所有 — 详见 [NOTICE](NOTICE)。
