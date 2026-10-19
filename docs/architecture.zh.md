# CoPet 架构

[English](./architecture.md)

CoPet 是一个 local-first 的桌面宠物客户端，面向 AI Agent CLI 工作流。它的核心思路很简单：Agent CLI 发出小型生命周期事件，CoPet 把这些事件转换成宠物行为，而用户可以替换宠物和音效资源，不需要改运行时。

架构上，CoPet 有意保持模块化。Agent 集成、宠物包、音效包、Skill 生成资产都通过稳定契约接入，而不是依赖写死的实现假设。

## 设计原则

- **默认本地优先** — runtime 状态、用户包、生成的 hooks、偏好设置都在用户机器上；CoPet 不依赖云服务观察 Agent 活动。
- **Agent 会话不能被 CoPet 阻塞** — hooks 是短生命周期调用，快速超时；CoPet 未运行时静默退出。
- **在边界处归一化** — 不同 Agent 的 hook 名称和 payload 形态不同，但应用内部只看一组小型共享事件词表。
- **资源包优先于硬编码资产** — 内置宠物和音效也使用与用户资产一致的包模型；应用扫描包，而不是在 UI 中写死资产列表。
- **Skill 是一等生成入口** — 宠物和音效生成通过 CoPet Skills 描述，让 Agent 可以生成资产并安装到应用运行目录。
- **UI 使用派生状态，不直接消费原始流** — 前端消费 app state、派生后的宠物状态和消息，不解释 Agent 原始 payload。
- **安全是架构边界** — 外部输入包括配置文件、本地 HTTP 事件、JSON manifest、图片和 MP3；每类输入在使用前都要解析、限定范围、校验。

## 系统形态

```text
Agent CLI hooks
  └─ 短生命周期 shell/plugin 调用
      └─ localhost event endpoint
          └─ Rust runtime core
              ├─ Agent adapter manager
              ├─ 事件归一化与宠物状态派生
              ├─ 配置、宠物包、音效包扫描
              └─ Tauri commands/events
                  └─ React 宠物窗口 + 设置窗口
```

Tauri 应用是唯一长期运行的进程。Agent CLI 只知道调用本地 helper 或插件。如果应用不可用，Agent 会话应当像 CoPet 不存在一样继续运行。

## Runtime 事件模型

CoPet 把 Agent 活动看成一条小型事件流：提交提示、工具开始、工具结束、等待权限、会话停止、会话错误。各 Agent 专属的事件名会在 runtime 边界被转换为这组通用词表。

Rust Core 负责状态派生。它把事件映射成 thinking、editing、inspecting、waiting、celebrating、failed 等稳定 UI 概念。前端再把这些 Agent 派生状态与本地互动状态组合起来，例如悬停、点击、长按、拖拽和 idle 行为。

这个分离让 hook 代码保持很小且可替换。Hooks 只报告事实；应用决定这些事实如何表现。

## Agent 集成

每个支持的 Agent 都是一个 adapter。Adapter 知道该 Agent 的 hook 配置在哪里、如何判断 CoPet 是否已安装，以及如何只添加或移除 CoPet 自己拥有的条目。

共享 manager 负责 adapter 周围的工程策略：安全备份、尽可能原子写入、可执行文件检测、repair 即重新安装、首次启动时一次性自动安装。新增 Agent 时，理想情况是新增一个 adapter 和对应测试，而不是改宠物渲染或设置架构。

当前 adapters 覆盖 Claude Code、Codex、Antigravity、OpenCode、Cursor、Copilot CLI、Pi、Gemini、WorkBuddy。

## 资源包

宠物是 Codex 兼容包：manifest 加 spritesheet。CoPet 不要求宠物必须是像素风；只要包遵循 Codex 宠物格式，并提供预期动画行，就可以被选择。

音效与视觉资源分开建模。宠物可以携带自己的音效，用户也可以选择全局音效包。这样一个视觉宠物可以搭配不同声音身份，生成的音效包也可以跨宠物复用。

内置资产同样以包形式打包。用户资产位于 `~/.copet`，导入流程会先暂存预览再提交，避免损坏包直接进入活跃状态。

## Skill 支持

`skills/` 目录描述了 CoPet 面向 agentic workflow 的资产创建入口：

- `copet-gen` 通过 `$hatch-pet` 完成宠物生成与视觉 QA，然后把完成包安装到 `~/.copet/pets`。
- `copet-sound` 在 `~/.copet/sounds` 下创建全局 11 段式 MP3 音效包。

这让创意生成留在应用运行时之外。应用只需要理解包契约；Skills 负责生成质量、冲突安全 id、校验流程和安装。结果是 CoPet 可以支持新的生成角色和声音风格，而不需要把生成逻辑写进 app。

## 前端模型

前端有两个窗口：悬浮宠物窗口和设置中心。它们共享一个 app store，由 Tauri commands 初始化，并通过 Tauri events 保持同步。

宠物窗口刻意保持轻量：渲染当前包、组合动画层、播放所选音效、处理直接互动。设置窗口负责管理流程：选择宠物、导入包、开关 Agent 集成、选择音效包和修改偏好。

展示组件不直接拥有 Rust IPC。带状态的应用操作通过 hooks 或 command wrappers 执行，这样测试可以干净地 mock Tauri 层。

## 工程边界

Rust 负责 OS 集成、持久化、Agent hook 改写、runtime 事件处理、包扫描和原生窗口行为。React 负责互动体验、设置流程、动画组合和用户反馈。

跨边界通信保持窄接口：请求走 typed Tauri commands，状态变化走 Tauri events，资产走 package manifests。这让每层都更容易测试，因为每个边界都有小而稳定的契约。

## 质量策略

测试布局跟随架构边界：

- Rust 集成测试覆盖 runtime 行为、Agent adapters、配置持久化、包导入、音效扫描、i18n 和窗口策略。
- Playwright 测试覆盖前端流程、跨窗口同步、手势、动画分层、音效和设置行为。
- 包与 Skill 文档描述生成资产的输入/输出契约，让新内容可以在不修改应用代码的情况下被校验。

修改架构时，优先加强这些契约，而不是增加特例。CoPet 在 Agent 专属细节留在 adapters、生成资产细节留在 Skills、runtime 维持小而稳定事件语言时，最容易维护。
