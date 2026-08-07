# Windows 已登录浏览器烟测：两小时重跑手册

状态：2026-08-07 已完成实跑，持续维护  
记录日期：2026-08-07  
适用仓库：`D:\project\nebula\codex-artifact-runtime`

本文是新会话的执行入口。背景和跨机器问题见
[`windows-browser-and-cross-machine.md`](windows-browser-and-cross-machine.md)。执行者必须先读仓库根目录
`AGENTS.md` 和本文，然后按闸门顺序运行，不重新发明浏览器接入方案。

## 0. 结论

可以把下一轮严格限制在两小时内，但这里的“两小时内”表示：在 120 分钟前得到“成功产物”或“明确的失败报告并停止”，不表示 GitHub、登录、MFA、网络、Windows 安全策略和录像组件一定都会成功。

本手册能直接避开或显著降低上一轮的主要问题：

- 不再接管、复制或强制关闭用户日常 Edge Profile，避开 Profile 锁、Cookie 解密、Chrome/Chromium 默认 Profile 远程调试限制和误伤日常窗口。
- 不再在 CDP、Edge 扩展、Playwright MCP 与 `agent-browser` 之间切换。唯一正式链路是原生 JavaScript Code Mode → Capability Host policy → `agent-browser` 原生可执行文件。
- 每个阶段有独立超时、一次重试上限和停止条件；子进程退出、连接断开和诊断命令异常都必须失败返回，不能用无限等待表现成“仍在运行”。
- 录像前先做短录像预检；录像开始后重新获取页面句柄；录像后验证内容变化，而不是只看文件存在。
- Gateway 只接受 `thread/start`，并在注入 Dynamic Tools 后输出 `GATEWAY_READY` 回执；G0 再用两次无副作用的 Host 调用正向验证入口，不再因 Code Mode 能发现受禁用的嵌套工具而误判失败。
- 在 G0 通过受根目录约束的 `fs.readText` 确认目标文件；当前已确认存在的是仓库根 `.gitignore`。禁止 Fork、commit、push 或其他远端写入，避免再因猜测路径或账户无权限而误触 Fork。

不能完全消除的风险包括首次登录/MFA、GitHub 页面改版、账户权限、网络、WDAC/AppLocker、安全软件和浏览器/FFmpeg 版本差异。它们一旦触发，应在对应闸门停止，而不是改走第二套传输继续试。

## 1. 被取消会话的实际卡点

被取消的会话并没有在后台持续完成工作。最后阶段主要卡在“复用日常 Edge 登录态”这条路线：

1. 配置被改为直接指向 `C:\Users\li\AppData\Local\Microsoft\Edge\User Data`。
2. 为释放 Profile 锁，尝试正常关闭 Edge 进程；等待 20 秒后进程仍未退出。
3. 随后一个本应只读的进程检查又进入额外授权/等待，未继续产生有效结果。
4. 更早的多轮尝试还遇到过默认 Profile Cookie 无法可靠解密、CDP/扩展连接时好时坏、遗留 daemon 占用旧会话、TUI 已退出但上层仍等待、参数引号被拆开、错误页面/页面句柄和录像轨道等问题。

因此，原样重跑不能规避问题；按本文改成“项目专属持久 Profile + 失败即停”后，才会避开该卡点。此次取消是正确的，不需要恢复旧会话。

### 1.1 2026-08-07 实跑录像复盘

本轮最终完成了可验收的“打开根 `.gitignore` → 点击 **Edit this file** → 进入编辑页 → 点击
**Cancel changes** → 返回原文件页”录像，但此前两次正式录像暴露了以下问题。以后必须把这些问题
视为 G5/G6 的固定检查项，而不是临场补救。

| 问题 | 实际表现 | 根因或误判 | 固定规避方式 |
| --- | --- | --- | --- |
| 只验技术指标，没有验操作语义 | 首条正式录像可解码、帧有变化，但看不清进入编辑和取消编辑，曾被错误报告为成功 | `decodable` 和帧变化只能证明视频不是坏文件，不能证明完成了演示 | G6 必须用录像内可见动作和录像外 snapshot 双重证明“文件页 → 编辑页 → 取消 → 文件页”；缺一步即 `VIDEO_SEMANTIC_FAILED` |
| 预检录像超出时长 | 首次短录像约 35.8 秒，虽然有内容仍不符合 3–10 秒预检 | 停止录像不够及时，且把“能播放”误当成“预检合格” | G5 同时检查时长、解码、抽样帧和画面变化；超出 3–10 秒按失败计入唯一重试 |
| 直接向 Monaco 编辑器 `fill` 产生脏草稿 | 文本没有按预期整体替换，页面出现未保存更改，取消或关页触发确认框 | 代码编辑器并非普通 textbox，通用 `fill` 的语义不稳定 | 本烟测不修改文本；演示只进入编辑页再取消。确需编辑时必须另设专门用例并验证编辑器内容及撤销结果 |
| 点击成功不等于导航已经稳定 | `click` 返回 `ok` 后等待约 1.8 秒，`listPages` 仍显示 blob URL；稍后同一页面才进入 edit URL | GitHub 客户端导航、页面标题和 Host 页面列表存在异步更新窗口 | 点击后用 `waitFor("Cancel changes")` 等待目标语义状态，再 `listPages` + `snapshot`；不得以固定 sleep 或一次 URL 读取判失败 |
| 过早停止录像 | 因上一项误判为未跳转，录像提前停止；稍后的 snapshot 已看到编辑页，但录像没有取消动作 | 以瞬时页面列表代替状态等待 | 只有确认返回文件页、重新出现 **Edit this file** 且 **Cancel changes** 消失后才能 `videoStop` |
| 原生确认框阻塞后续调用 | 关闭带脏状态的编辑页时出现“是否丢弃未保存更改”，后续新开的目标页也被该对话框阻塞 | 页面句柄从列表消失不代表底层原生 dialog 已解决 | 清理前优先点击页面内 **Cancel changes**；若 Host 明确报告该丢弃确认框，只能通过同一 Host 的受控按键/对话框能力确认，随后重新 snapshot 证明已解除；不得改用外部浏览器工具 |
| `closePage` 错误与空页面列表并存 | 关闭旧句柄返回 `Tab not found` 和 dialog warning，但随后 `listPages` 为空 | 旧 context、标签页登记和原生 dialog 生命周期不同步 | 同时记录 close 原始错误和最终 `listPages`；下次 attach 后先做无录像 snapshot，任何 dialog warning 都必须在录像前处理 |
| 页面标题滞后 | 取消后 URL 和 snapshot 已回到文件页，但 title 短时间仍含 `Editing` | SPA 导航后标题更新晚于主体 DOM | 最终状态以完整 URL、`.gitignore` 标题、**Edit this file** 出现、**Cancel changes** 消失四项联合判断，不能只看 title |
| 录像启动后页面句柄复制 | `videoStart` 后同时出现旧页和新的 current 页 | 录像会创建新 context | `videoStart` 后立刻重新 `listPages`/`snapshot`，优先选择完整 URL 匹配且 `current=true` 的新句柄；旧 `page_id` 和 `eN` 全部作废 |

本轮的关键教训是：**媒体健康检查是必要条件，不是业务验收**。报告 `SUCCESS` 前，执行者必须能
从录像和状态证据中逐项指出预期动作；用户肉眼看不到的操作不能用“帧有变化”替代。

### 1.2 Windows 故障总表

下表把本手册、历史跨机器记录以及本轮实跑问题归并为一个执行视角的总表。背景和设计讨论仍保留在
[`windows-browser-and-cross-machine.md`](windows-browser-and-cross-machine.md)，实际烟测以本表和 G0–G8 为准。

| 边界 | 已遇到或需重点防范的问题 | 统一处理原则 |
| --- | --- | --- |
| Windows 安全策略 | WDAC/Application Control 曾以 4551 阻止新编译 Host；AppLocker、安全软件也可能拦截或锁文件 | 启动预检应区分缺失、权限和策略拦截；使用组织认可的签名/部署，不临时换第二套传输 |
| 本地程序启动 | npm 常暴露 `.cmd` shim；nvm-windows、CPU 架构和安装布局会改变原生 exe 路径 | Host 解析并记录包内原生 exe，以“可执行文件 + 参数数组”启动，禁止拼 shell 命令 |
| 子进程生命周期 | daemon 继承 stdout/stderr 管道，直接子进程已退但 Host 仍等 EOF；异常时遗留捕获文件 | 输出写入有界临时文件，只等待直接子进程，设置阶段超时并按严格命名清理遗留 |
| Gateway/线程入口 | 恢复旧线程不会获得 `thread/start` 注入；枚举到 shell/MCP 曾被误判为入口未关闭 | 要求 `GATEWAY_READY` 和两个 Dynamic Tools 正向探针；工具可见性只代表发现，不代表授权 |
| 工作目录与文件边界 | 盘符、反斜杠、ADS、保留名、junction、symlink、大小写和错误同名文件会导致越界或选错目标 | 使用逻辑根和 `fs.readText` 验证根 `.gitignore`；拒绝绝对路径、穿越和 symlink 越界 |
| Profile 与登录态 | 日常 Edge Profile 锁、Cookie 解密、默认 Profile 远程调试限制、跨机复制登录态失败 | 每台机器使用项目专属持久 Profile，headed 首次人工登录；不复制、不关闭、不接管日常 Edge |
| 浏览器接入 | CDP、扩展、Playwright MCP 与原生路径混用；扩展桥的 `run-code`/`eval`/`mousewheel` 曾挂起 | 正式路线只用单一 Host 管理的原生 `agent-browser`；结构化操作、有界超时，不临时切后端 |
| 页面所有权 | 重复标签页、错误 current 页、全白录像轨道、旧 session/daemon | 按 session 所有权清理；每次用完整 URL、current 状态、snapshot 和 screenshot 重新选页 |
| 元素引用 | 导航、重渲染、录像换 context 后旧 `eN` 失效；iframe `fNeM` 记录与实现不一致 | 每次状态变化后重新 snapshot；iframe 引用在修复并复测前视为不支持 |
| 页面状态等待 | 点击返回成功但 SPA 导航、URL、title 或主体 DOM 尚未一致 | 等待目标语义文本，再联合 URL 和 snapshot 验证；只做一次同路径重试，不无限 sleep |
| 原生 dialog | 未保存草稿的确认框可跨页面调用继续阻塞，页面列表可能无法反映它 | 录像前清除已知 dialog；只用同一 Host 的受控能力处理，并在处理后重新 snapshot |
| 录像依赖 | FFmpeg/codec 缺失、版本差异、录像停止慢、视频全白或静止 | 浏览器健康与录像健康分开；先做 3–10 秒预检，再以同一 Host `videoInspect` 验收 |
| 录像语义 | 文件存在、可解码、帧变化，但没有录到约定动作 | 技术指标与业务动作双重验收；正式录像必须完整覆盖起点、动作、结果和可撤销状态 |
| 环境差异 | Edge/Chromium、CLI、FFmpeg、DPI、语言、账户权限、A/B 页面、代理/VPN/证书不同 | 记录版本；使用语义定位；白名单最小化；MFA/CAPTCHA 交给用户；按阶段设置超时 |
| 清理与报告 | TUI/Host 已退但上层继续等；失败被写成“仍在运行”；关错日常浏览器 | 失败立即返回具体闸门、原始错误和重试次数；只关闭任务页面/进程，保留用户工作区和专属 Profile |

## 2. Code Mode 与故障边界

当前源码会用 `--enable code_mode --enable code_mode_only` 启动官方 App Server。
Codex 官方源码把 `code_mode_only` 定义为：模型可见工具只保留 Code Mode 的 `exec`、`wait`
入口。因此，从本项目 Gateway 启动的会话已经是“单一模型入口”；直接打开的普通 Codex 会话不受
这个项目启动参数约束。

“单一入口”不等于把入口后的所有原生能力从 Codex 进程中卸载。Code Mode 的能力目录仍可能列出
`shell_command`、`apply_patch` 或本机配置的 MCP；那是嵌套能力发现，不是新的模型顶层入口，也不
代表本次任务已获准调用。当前 Gateway 另外按名称关闭本机配置中已有的
`openaiDeveloperDocs` 和 `playwright-browser` MCP，但没有“卸载所有原生嵌套工具”的通配实现。
因此，**不能再把“目录中看见这些名字”作为入口闸门失败条件**，也不能声称它们已从进程中彻底
移除。浏览器任务的授权规则仍是：只调用 Gateway 注入的 Dynamic Tools，每次调用都进入
Capability Host policy；发现到的其他嵌套能力不得用于本轮烟测。

本轮改用可执行的正向入口证明：

1. 启动参数带 `--require-new-thread`；如果 TUI 尝试 `thread/resume`，Gateway 立即返回
   `ENTRY_NOT_CLOSED`，避免恢复到没有 `thread/start` 注入的旧线程。
2. Gateway 只在拦截到 `thread/start` 并成功写入 Dynamic Tools 后输出
   `GATEWAY_READY thread=start ... code_mode_only=true`。
3. 新会话的前两个调用必须是项目 Dynamic Tools：`approval.getPolicy({})` 和
   `fs.readText({root:"workspace", path:"AGENTS.md"})`。二者均成功，才证明本会话能发现项目能力、
   请求确实到达当前 Host、工作区逻辑根可用。任一能力不存在或调用失败，才返回
   `STOPPED / ENTRY_NOT_CLOSED`。

Codex Dynamic Tools 由 App Server 在 `thread/start` 请求中接收；普通会话或旧线程恢复不能替代
这一步。入口验证不能再要求会话用已禁止的 shell 或直接 MCP 去反查 Gateway 源码，否则会形成
“必须绕过规则才能证明没有绕过规则”的死结。

现有证据没有显示 Codex 原生 JavaScript Code Mode 协议本身存在功能性故障。已知故障主要在以下边界：

```text
Codex 原生 JavaScript Code Mode
  -> 由同一 capability catalog 生成的 Dynamic Tools / TSX tools
  -> Capability Host policy（每次调用都鉴权）
  -> Windows 原生进程启动与输出/超时管理
  -> agent-browser daemon / 浏览器会话 / 页面 / 录像
  -> GitHub、网络、登录和本机安全策略
```

工具可见性只是发现，不是授权。不得加入第二个 Code Mode runtime、第二套工具传输或绕过 Host policy 的浏览器直连。Host 启动本地程序时仍必须使用“可执行文件 + 参数数组”，不得构造 PowerShell、`cmd.exe` 或其他 shell 命令字符串。

## 3. 同类工具如何规避这些问题

主流工具采用的是“隔离状态 + 明确生命周期 + 有界等待”，并不存在一个能消除所有故障的万能插件。

| 做法 | 官方工具中的实现 | 解决的问题 | 对本项目的结论 |
| --- | --- | --- | --- |
| 项目专属持久 Profile | `agent-browser --profile <path>`；Playwright `launchPersistentContext(userDataDir)`；Playwright MCP 按 workspace hash 使用独立 Profile | 保留 cookies、localStorage、IndexedDB 等，同时避开日常 Profile 锁和并发争用 | **本轮采用**；每台电脑首次 headed 登录一次 |
| 认证状态文件 | Playwright `storageState`；`agent-browser state save/load` | 比复制整个 Profile 更轻量，可在隔离上下文中恢复 cookies/localStorage | 可作为以后由 Host 管理的导入能力；状态文件含会话秘密，当前不临时扩展能力 |
| 语义定位与动作前重新解析 | Playwright locators、auto-wait/actionability；`agent-browser snapshot/find` | 降低 DOM 重渲染、动画遮挡和元素暂不可用造成的失败 | 每次导航、刷新、明显重渲染或录像换 context 后重新 snapshot；不复用旧 `eN` |
| 连接已有浏览器 | Playwright/Puppeteer `connectOverCDP`；`agent-browser --cdp/--auto-connect` | 直接使用已经登录的页面 | 只作人工排障或一次性认证导入，不作为本轮默认路径；Playwright 官方称 CDP 连接比其原生协议低保真 |
| 浏览器扩展中继 | Playwright MCP Extension 让用户选择现有 tab，并可用 profile token 减少重复授权 | 能利用日常浏览器登录态，用户可显式选择页面 | 有额外扩展、relay、审批和后台生命周期；本项目又禁止第二传输，因此本轮不采用 |
| 云端浏览器 Context | Browserbase Context 持久保存 cookies、IndexedDB、service workers 等；其他云浏览器也使用类似持久 Profile | 隔离、远程运行、统一版本和录制基础设施 | 是另一种部署模式，涉及外发登录态、成本和新后端；不在本次本机验证范围 |

几个对本轮特别重要的官方事实：

- Chrome 从 136 起不再接受对默认 Chrome 数据目录使用远程调试开关，并要求非默认 `--user-data-dir`；官方也建议调试使用自定义目录。Edge 的具体策略仍以 Microsoft 实现为准，但这足以说明“依赖 Chromium 日常默认 Profile 远程调试”不是稳健的跨机器基线。
- Playwright 明确警告不要自动化默认 Chrome User Data，建议建立单独目录；同一 User Data Directory 也不能被两个浏览器实例并发使用。
- `agent-browser` 官方把“持久自定义 Profile”“按名称复制已有 Chrome Profile”“CDP 接管”“状态文件”分成不同模式；它们不是可在失败时随意混用的等价重试。
- Playwright 的 locator 会在每次动作时重新解析 DOM，并在点击前检查可见、稳定、可接收事件和启用状态。这里借鉴的是其工程策略，不是在本项目中引入 Playwright MCP。

## 4. 本轮唯一选定的浏览器方案

启动时直接加载 `artifacts/browser/bindings.two-hour-rerun.json`。该文件由
`examples/windows-browser/bindings.windows.json` 派生并放在 Git 忽略目录中；不能在 Gateway
和 Codex 会话已经启动后才生成或替换 binding，因为当前进程不会热重载它。它采用以下模式：

- headed Edge/Chromium；
- 仓库范围内的专属持久目录 `artifacts/browser-profile`；
- 稳定且唯一的 `sessionName`；
- `autoConnect: false`，不设置 `cdpEndpoint`；
- `allowedHosts` 只含本任务必要的 `github.com` 和 `github.dev`；
- 由 Host 把 npm shim 解析为包内原生 `agent-browser-win32-x64.exe`，再以参数数组启动；
- 只使用 `attach/listPages/openPage/navigate/snapshot/click/fill/press/read/scroll/waitFor/screenshot/videoStart/videoStop/videoInspect` 等受控 operation。

当前的 `artifacts/browser/bindings.edge-login.json` **不能直接用于重跑**，因为它仍指向用户日常 Edge 的原始 User Data：

```json
"profileDirectory": "C:\\Users\\li\\AppData\\Local\\Microsoft\\Edge\\User Data"
```

本轮已生成的安全 binding 使用 `../browser-profile`；该路径相对 binding 所在目录解析，最终是
仓库内的 `artifacts/browser-profile`。不得为了释放锁而关闭、结束或移动用户日常 Edge；不得复制
Default Profile；不得把浏览器 Profile、认证状态或 token 提交到 Git。

## 5. 两小时执行契约

计时从新会话开始读取本文时算起，包含授权等待和人工登录时间。执行者在每个闸门结束时记录本地时间和结果。

| 墙钟时间 | 闸门 | 必须得到的结果 |
| --- | --- | --- |
| 0–10 分钟 | G0：入口与边界确认 | 得到 Gateway 回执；用 Host 读取 `AGENTS.md`、本文和目标文件；不得覆盖用户改动 |
| 10–25 分钟 | G1：静态预检 | 用 Host 读取并核对 binding：专属 Profile、允许域名和工作区根正确；浏览器/录像依赖由受控调用实测 |
| 25–40 分钟 | G2：会话启动 | 只清理本任务拥有的旧 session/daemon；headed 专属浏览器可启动并返回 `session_id` |
| 40–55 分钟 | G3：登录闸门 | GitHub 页面明确显示已登录；若需要 MFA/CAPTCHA/Passkey，只等待用户在可见窗口完成 |
| 55–70 分钟 | G4：页面闸门 | 唯一目标页 URL/标题正确；snapshot 和 screenshot 正常；确认目标是根 `.gitignore` |
| 70–85 分钟 | G5：短录像预检 | 录制 3–10 秒可见滚动；`videoStop` 成功；WebM 可解码且至少两个抽样帧不同 |
| 85–105 分钟 | G6：正式演示 | 在同一受管会话完成预定、可撤销的浏览/编辑演示并录像；无 Fork/commit/push |
| 105–115 分钟 | G7：产物验收 | 校验 URL、时长、关键帧变化、截图和逻辑产物路径；不完整产物不算成功 |
| 115–120 分钟 | G8：清理与报告 | 停止新实验；只关闭任务拥有的页面/进程；提交成功或失败报告 |

硬规则：

- 120 分钟是停止线，不是“再试一次”的开始线。到点必须结束并报告。
- 任一闸门最多执行两次（初次 + 一次同路径重试）。第二次失败就停止，不切换传输、浏览器或 Profile 模式。
- 单次进程/Host 启动最多 30 秒；浏览器连接最多 30 秒；普通页面动作最多 15 秒；导航最多 30 秒；`videoStop` 最多 60 秒。禁止把超时设为 0。
- 100 分钟时如果正式演示尚未开始，直接转失败报告，给验收和清理留时间。
- 每 15 分钟至少向用户报告一次当前闸门、已用时间和是否仍在预算内。任何工具调用本身不得无界等待。

## 6. 分闸门执行步骤

### G0：证明入口并保护工作区

1. 启动 Gateway 的 PowerShell 窗口应在新线程首条消息后出现
   `GATEWAY_READY thread=start ... code_mode_only=true`。若出现 `ENTRY_NOT_CLOSED`，停止并用同一
   Gateway 命令新建线程，不能恢复旧线程。
2. Code Mode 的前两个调用必须依次为 `approval.getPolicy({})` 和
   `fs.readText({root:"workspace", path:"AGENTS.md"})`。期望 policy 的允许域名只有
   `github.com`、`github.dev`，且能读到本仓库指导。能力缺失、根不匹配或调用失败时返回
   `STOPPED / ENTRY_NOT_CLOSED`。仅仅发现 shell、apply_patch 或 MCP 名称不算失败，但本轮不得调用。
3. 再用 `fs.readText` 完整读取本文、`artifacts/browser/bindings.two-hour-rerun.json` 和根
   `.gitignore`。当前工作区已有用户未提交改动和未跟踪文件，全部保留；本轮不要 reset、checkout、
   clean 或覆盖。`fs.writeText` 是只创建不覆盖，也不得用于改写现有文件。
4. 本文编写时已确认目标是根 `.gitignore`，不是 `docs/.gitignore`。执行时以上一步读取成功和目标
   URL 双重确认；不得自行猜测或替换目标。

### G1：配置与依赖预检

1. 核对 G0 读取的 `artifacts/browser/bindings.two-hour-rerun.json`：Profile 最终为仓库内
   `artifacts/browser-profile`，且没有 `cdpEndpoint`、没有 `autoConnect: true`。同时核对
   `approval.getPolicy` 返回与 binding 一致；不一致表示启动时加载了别的 binding，停止并重启。
2. 允许域名仅为 `github.com`、`github.dev`；产物目录使用相对工作区的 `artifacts/browser`。
3. 验证 Host 最终解析到原生 `.exe`，但不要绕过 Host 直接让 Code Mode 调 CLI，也不要通过 `.cmd` shell 执行。
4. 分开报告“浏览器操作健康”和“录像健康”。缺少 FFmpeg 时浏览器截图仍可测试，但 G5 必须失败停止。

### G2：启动唯一会话

1. 只根据明确的 session 名、PID 所有权或本轮生成的元数据处理任务遗留进程；不按进程名批量结束 Edge。
2. 启动 native Code Mode/Capability Host。若下层 TUI 或 Host 进程退出，上层必须立即返回退出码和 stdout/stderr，不得继续等待网关。
3. 调用 `webBrowser.attach`，再 `listPages`。保留一个明确的任务页；有重复页时按完整 URL 和当前 screenshot 选择，不凭页面序号猜。

### G3：登录

1. 打开 GitHub 登录状态检查页或目标仓库。
2. 已登录则继续；未登录时让用户在 headed 专属浏览器中人工登录。不要让 agent 读取密码、Cookie、token、Passkey 或验证码。
3. 15 分钟登录窗口结束仍未登录，报告 `LOGIN_NOT_READY` 并停止。下一次会话可复用同一个专属 Profile，无需重做前面失败路线。

### G4：目标页

本轮默认目标是：

```text
仓库：https://github.com/Nebula-YW/codex-artifact-runtime
文件：.gitignore（仓库根目录；已由本地 HEAD 验证存在）
演示：只做可撤销的本地网页编辑/浏览，不提交、不推送、不创建 Fork
```

如果 GitHub 因账户无写权限只提供 Fork，立即取消该动作。需要演示编辑时，可使用 `github.dev` 的本地编辑器打开准确路径，完成输入后撤销或丢弃本地改动；不能确认路径时宁可停止，不能改到其他同名文件。

每次导航、刷新、切换标签页或明显 DOM 更新后重新 `listPages` 和 `snapshot`。只使用最新返回的 `page_id` 与元素引用。

### G5：短录像预检

1. 在已确认的目标 `page_id` 上调用 `videoStart`。
2. 录像会建立新 context，立刻重新 `listPages`，重新选择完整 URL 匹配的当前页并重新 snapshot。
3. 做一次可见滚动或展开/收起等只读动作，持续 3–10 秒，然后按 `session_id` 调 `videoStop`。
4. 将 `videoStop` 返回的 `artifact_id` 传给同一 Host 的只读 `videoInspect`；确认 `decodable=true`、时长合理、`sampled_frames>=3`、`distinct_frame_hashes>=2` 且 `frames_changed=true`。不得把产物路径交给 shell 或其他工具补验。空白、静止、错误标签页或只有约 1 秒的录像都算失败。
5. 初次失败只允许按相同路径重试一次；仍失败就报告 `VIDEO_PREFLIGHT_FAILED`，不得进入正式演示。

### G6–G8：正式演示、验收和清理

正式录像开始后再次刷新页面句柄。本用例的固定动作是：

1. 在录像中保留足够时间显示根 `.gitignore` 文件页。
2. 点击 **Edit this file**，用 `waitFor("Cancel changes")` 等待编辑页真正就绪，再重新
   `listPages`/`snapshot` 并短暂停留，使录像能看清页面状态。
3. 不修改编辑器内容，点击 **Cancel changes**。若出现明确的丢弃确认框，只能通过同一 Host 的
   受控能力处理。
4. 重新 snapshot，确认完整 URL 回到 `/blob/main/.gitignore`、页面有 `.gitignore` 标题和
   **Edit this file**，且 **Cancel changes** 已消失。完成这些检查后才能 `videoStop`。

动作前确认当前 URL，动作后用 snapshot/screenshot 证明页面发生了预期变化。完成后停止录像，
抽样检查开头、中间和结尾画面。`videoInspect` 的解码、时长和帧变化指标全部通过仍不充分；如果录像
没有清楚包含上述完整动作链，必须报告 `VIDEO_SEMANTIC_FAILED`，不能返回成功。

只关闭本任务创建的页面和受管 session。保留专属 Profile 以便以后复用登录态；产物留在 `artifacts/browser`。不关闭用户日常 Edge，不清理工作区其他未提交文件。

## 7. 必须立即停止的条件

出现以下任一条件，记录证据并停止当前路线：

- 实际工作目录不是仓库根，且无法在 10 分钟内纠正；
- Gateway 拒绝旧线程，或 `approval.getPolicy` / `fs.readText` 不存在、失败、返回的 policy 与本轮 binding 不一致；
- binding 指向日常 Edge/Chrome User Data；
- Profile 已被其他实例占用，且该实例不是本任务拥有；
- Host/TUI 已退出而上层仍等待，或同一调用超过阶段超时；
- CDP、扩展、Playwright MCP 或另一浏览器后端被提议作为临时绕行；
- 登录需要用户完成但 15 分钟内没有完成；
- 页面要求 Fork、commit、push、付费、发布或其他未授权外部副作用；
- 短录像预检连续两次失败；
- 到 100 分钟仍未开始正式演示；
- 到 120 分钟，无论状态如何。

## 8. 成功标准与失败报告

只有同时满足以下条件才算成功：

- 全程使用原生 JavaScript Code Mode 和单一 Capability Host 传输；
- Gateway 接受的是 `thread/start`，且 G0 两个 Host 正向探针成功；
- 使用项目专属 Profile，GitHub 登录状态可用；
- 操作的是准确目标 URL 和仓库根 `.gitignore`，没有远端写入；
- 录像文件可解码、时长合理、关键帧有变化并包含预期操作；
- 没有覆盖工作区既有改动，没有遗留无法解释的任务进程；
- 总墙钟时间小于 120 分钟。

失败报告不得写“还在运行”，而应使用以下格式：

```text
状态：SUCCESS 或 STOPPED
耗时：__ 分钟
停止闸门：G__
错误类别：CWD / HOST_START / PROFILE_LOCK / LOGIN_NOT_READY /
          PAGE_MISMATCH / NAVIGATION_NOT_SETTLED / DIALOG_BLOCKED /
          VIDEO_PREFLIGHT_FAILED / VIDEO_SEMANTIC_FAILED / POLICY / EXTERNAL
最后一个成功操作：__
原始错误或退出码：__
已执行重试：0 或 1
产生的产物：__
保留的任务进程：无 / 明确 PID 与原因
下次只需从哪里继续：__
```

## 9. 新会话可直接粘贴的指令

不能先打开普通 Codex 会话再粘贴 Prompt；那样不会自动获得本项目的 `code_mode_only` 和 Host
边界。请先在 PowerShell 运行以下命令，让当前源码 Gateway 创建新会话（不要改用已安装的旧版
`codex-artifact.exe`）：

```powershell
$env:Path = "$env:LOCALAPPDATA\Microsoft\WinGet\Links;$env:Path"
cargo run --manifest-path 'D:\project\nebula\codex-artifact-runtime\Cargo.toml' `
  -p codex-artifact-cli --bin codex-artifact -- run `
  --catalog 'D:\project\nebula\codex-artifact-runtime\examples\windows-browser\capabilities.json' `
  --bindings 'D:\project\nebula\codex-artifact-runtime\artifacts\browser\bindings.two-hour-rerun.json' `
  --codex-bin 'C:\nvm4w\nodejs\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe' `
  --require-new-thread --allow-side-effects -- -C 'D:\project\nebula\codex-artifact-runtime'
```

出现由 Gateway 启动的新 Codex TUI 后，再粘贴：

```text
请执行 Windows 已登录浏览器烟测，并把
docs/troubleshooting/windows-browser-two-hour-rerun.md 作为本轮唯一运行手册。

不要通过枚举嵌套能力判断入口是否关闭：Code Mode 内即使能发现 shell_command、apply_patch、
OpenAI Docs MCP、Playwright MCP 或其他名字，也只表示发现，不表示本轮授权，不是入口失败。
不得调用这些能力完成烟测，也不得用它们反查源码。

入口采用正向验证。你的前两个工具调用必须依次是本项目 Dynamic Tools：
1. approval.getPolicy({})；
2. fs.readText({root:"workspace", path:"AGENTS.md"})。
若任一工具不存在、调用失败，或 policy 的 allowed_hosts 不是仅含 github.com 与 github.dev，立即返回
STOPPED / ENTRY_NOT_CLOSED，不启动浏览器。二者成功后，用 fs.readText 完整读取
docs/troubleshooting/windows-browser-two-hour-rerun.md、
artifacts/browser/bindings.two-hour-rerun.json 和根 .gitignore，再严格按 G0–G8 执行。
启动 Gateway 的终端应同时出现 GATEWAY_READY；若出现 ENTRY_NOT_CLOSED，说明不是新 thread/start，
立即停止，不恢复旧线程。

从读取文档开始计算 120 分钟墙钟上限。严格按 G0–G8 执行，每个闸门最多一次同路径重试；
100 分钟还未开始正式演示就停止，120 分钟必须返回 SUCCESS 或 STOPPED 报告，不能继续等待。
每个闸门结束记录本地时间，每 15 分钟给我一次简短进度。

只使用 Codex 原生 JavaScript Code Mode、同一 capability catalog 和单一 Capability Host；
所有浏览器调用都经过 Host policy。不得引入 Playwright MCP、浏览器扩展或第二套工具传输，
不得绕过 Host 直接操作 agent-browser。Host 调本地程序必须是可执行文件加参数数组，不构造 shell 命令。

只使用启动时已经加载的 artifacts/browser/bindings.two-hour-rerun.json 和其中受忽略的项目专属
持久 Profile；不得在会话启动后修改 binding 并假定它会热重载。不得使用、复制、关闭或结束我的日常 Edge Profile/进程，
不得使用 artifacts/browser/bindings.edge-login.json 当前指向的日常 Edge User Data。
保留当前工作区所有未提交和未跟踪内容。

目标仓库是 https://github.com/Nebula-YW/codex-artifact-runtime，准确目标文件是仓库根目录 .gitignore；
开始时先用 fs.readText 验证它存在，不得自行替换成 docs/.gitignore 或其他同名文件。
完成可撤销的本地网页编辑/浏览录像演示；不 Fork、不 commit、不 push、不创建 PR，也不做任何远端写入。
如果账户无写权限，可使用 github.dev 的本地编辑器并在结束前撤销或丢弃本地编辑。

录像前必须做 3–10 秒短录像预检；videoStart 后重新 listPages/snapshot 获取新页面句柄；
videoStop 后必须把返回的 artifact_id 传给同一 Host 的 videoInspect，正式结果必须验证
decodable=true、时长合理、sampled_frames>=3、distinct_frame_hashes>=2 且 frames_changed=true。
失败时按文档模板报告具体闸门，
不要把“等待中”当作结果，也不要改走其他浏览器接入方案。
```

## 10. 官方参考资料

以下资料于 2026-08-07 核对：

- [`agent-browser` README：持久 Profile、Profile 快照、state、session 与 CDP](https://github.com/vercel-labs/agent-browser/blob/main/README.md)
- [Playwright：持久 Context 与 CDP 连接的限制](https://playwright.dev/docs/api/class-browsertype)
- [Playwright：认证状态](https://playwright.dev/docs/auth)
- [Playwright：Locator](https://playwright.dev/docs/locators)
- [Playwright：Auto-waiting / Actionability](https://playwright.dev/docs/actionability)
- [Playwright MCP：Profile 与状态模式](https://playwright.dev/mcp/configuration/user-profile)
- [Playwright Extension：连接已有浏览器页面的显式中继方式](https://github.com/microsoft/playwright/blob/main/packages/extension/README.md)
- [Chrome for Developers：Chrome 136 默认数据目录远程调试策略变化](https://developer.chrome.com/blog/remote-debugging-port)
- [Browserbase：持久 Context](https://docs.browserbase.com/platform/browser/core-features/contexts)
- [Browser Use：Browser Profile 参数](https://docs.browser-use.com/open-source/customize/browser/all-parameters)
