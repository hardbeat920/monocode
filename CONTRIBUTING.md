# 贡献指南

MonoCode 目前还处于早期阶段，我是唯一的维护者，因此小而聚焦的改动比大而宏大的改动更容易被合并。除此之外，大门始终敞开 -- 非常欢迎提交 bug 报告和修复。

请暂时不要提交添加新 provider 的 PR。现有的适配层还需要在一些模式上达成一致，新增适配器只会复制当前已有的做法。参见 [新 provider](#新-provider)。

## 运行项目

你需要 Node.js 20+、当前稳定的 Rust 工具链，以及至少一个已安装并登录的 provider CLI：

- [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
- [Codex](https://developers.openai.com/codex/cli) - `codex login`
- [Cursor CLI](https://cursor.com/cli) - `agent login`
- [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` 然后 `grok login`
- [OpenCode](https://opencode.ai) - `opencode auth login`
- [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
- [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
- [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` 然后 `fx login`

支持 macOS 和 Linux 平台。在 Debian/Ubuntu 上，`npm run setup:linux:deb` 会安装 Tauri 的原生构建依赖。

```bash
npm install
npm run tauri dev
```

一个 provider 就够了。MonoCode 在启动时会探测每个 CLI，找不到的会自动禁用，并给出安装提示，所以缺少 Codex 不会影响你开发其他功能。

## 项目结构

- `src/chrome/` - 窗口框架：标题栏、侧边栏、编辑器、标签页、模型选择器
- `src/surfaces/` - 标签页内的面板：对话记录、文件编辑器、diff、终端
- `src/lib/harness/` - 每个 provider 一个适配器，以及它们接入的注册中心
- `src-tauri/src/` - Rust 端：PTY、文件系统和 git、会话存储、原生窗口

如果你想修复一些实际问题，`src/lib/harness/` 是最好的起点。每个 provider 都有一个适配器（`claudeAdapter.ts`），实现了 `registry.ts` 中定义的共享 `HarnessAdapter` 生命周期，还有一个协议模块（`claudeProtocol.ts`），负责将 CLI 的输出转换为 MonoCode 自身的事件类型。协议模块是纯函数，旁边就有单元测试，所以即使只安装了 Claude Code，你也可以修复 Codex 的解析 bug。以上是针对我们已有的 provider -- 请暂时不要添加新的。

## 提交前检查

```bash
npm run check
```

这会运行 CI 中的所有检查：vitest、`tsc --noEmit`、`cargo fmt`、`cargo clippy` 和 `cargo test`。如果本地通过，在 GitHub 上也应该通过。`npm run check:web` 和 `npm run check:rust` 可以分别运行前端和后端的检查，适用于你只修改了一侧的情况。

## 新 provider

我正在暂停添加新的适配层，直到现有的适配层在以下方面达成一致：会话生命周期、目录探测、用量统计、审批流程，以及斜杠命令和技能的接入方式。新增 provider 的 PR 目前会被关闭，即使代码质量很好。对 Claude、Codex、Cursor、Grok、OpenCode、Pi、omp 和 fx 的修复、测试和协议 bug 修复仍然是最好的贡献方式。

暂停解除后，本节将被移除。

## Pull 请求

一个 PR 只做一件事，并说明改了什么以及为什么。[PR 模板](.github/pull_request_template.md) 涵盖了其余内容。如果涉及 UI 变更，附上变更前后的截图会很有帮助。

对于影响产品方向的改动 -- 新的界面层、新的 provider 行为、改变应用结构的重构 -- 请先开一个 issue。这不是在设置门槛，我只是希望你在动手之前就能听到"我已经做了一半了"，而不是做完之后才知道。新 provider 是例外：在上面的暂停解除之前，即使有对应的 issue，也不要提交适配器代码。

我可能会关闭一个 PR、要求你缩小范围，或者最终用不同的方式实现这个想法。这是关于范围和时机的决定，不是针对你或你工作的质量。

请保持友善：[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。安全报告：[SECURITY.md](SECURITY.md)。
