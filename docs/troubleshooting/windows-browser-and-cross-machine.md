# Windows 浏览器能力接入与跨机器运行问题总结

状态：持续补充中  
首次记录：2026-08-06

本文记录 Windows 浏览器能力接入和实机烟测中已经遇到的问题，以及换到其他电脑后需要重新检查的环境风险。它不是 Code Mode 协议本身的故障清单；当前已知问题主要位于 Capability Host、浏览器驱动、浏览器会话和本机运行环境这一段链路。

下一轮要在两小时内重跑时，直接使用
[`windows-browser-two-hour-rerun.md`](windows-browser-two-hour-rerun.md)；该手册包含固定技术路线、阶段闸门、停止条件和可复制到新会话的完整指令。
其中“Windows 故障总表”是当前统一的执行清单，并已包含 2026-08-07 实跑发现的异步导航、
录像语义误判、原生确认框残留和页面状态不同步问题；本文保留背景、跨机器风险和设计说明，不再
另建平行的烟测故障清单。

## 1. Windows 端已遇到或重点关注的问题

### 1.1 Windows Application Control 阻止新编译的 Host

新编译的 Host 可执行文件曾被 Windows Application Control 以错误 `4551` 拒绝运行，即使本地签名状态显示有效也未获放行。结果是 Host 无法启动，后续浏览器能力完全不可用。

本次为了完成实机录像验证，临时使用了系统已经授权的官方 Playwright CLI 和 Edge 扩展会话。该路径只用于验证，不应成为产品正式运行时的第二套工具传输或替代 Code Mode。

后续应：

- 在启动时预检 Host，区分文件缺失、权限不足和 Application Control 拦截。
- 在部署说明中覆盖 WDAC、AppLocker 和安全软件白名单。
- 使用符合目标组织策略的签名和部署方式。
- 继续保持原生 JavaScript Code Mode、单一 Capability Host 和单一工具传输。

### 1.2 Windows npm shim 与原生进程启动

Windows 全局安装通常暴露 `agent-browser.cmd`。项目要求本地程序必须通过“可执行文件 + 参数数组”调用，不能构造 shell 命令，也不能借助 `cmd.exe` 或 PowerShell 执行 `.cmd`。

当前 Host 会尝试把 npm shim 解析为包内原生 `agent-browser-win32-x64.exe`。换机风险包括 Node/npm 安装位置不同、nvm-windows 或其他包管理器目录布局不同、ARM64 架构不匹配，以及依赖升级后内部目录变化。

后续应输出最终解析路径，在失败时给出安装布局和 CPU 架构诊断，并补充非 nvm-windows 与 Windows ARM64 验证。

### 1.3 daemon 继承输出管道导致调用不结束

`agent-browser` 会启动长生命周期 daemon。在 Windows 上，daemon 可能继承匿名 `stdout/stderr` 管道，导致短生命周期 CLI 已退出，但 Host 仍等待 EOF，外部表现为命令挂起。

当前实现改为使用有大小限制的临时文件捕获输出，只等待直接子进程，并设置调用超时和 8 MiB 输出限制。异常终止仍可能遗留 `.agent-browser-call_*.stdout/stderr` 文件；工作区中已经出现过此类遗留文件。

后续可在 Host 启动时按严格命名和文件年龄清理遗留捕获文件，并补充超时、异常退出和句柄继承测试。

### 1.4 Edge 扩展桥接中的不稳定命令

`run-code`、`eval` 和 `mousewheel` 在当前 Edge 扩展桥接会话中出现过挂起，因此没有用于录像关键路径。本次稳定路径使用 `find`、`click`、`press`、`screenshot` 和 `video-stop` 等结构化操作。

正式能力应继续限制为结构化操作，不开放任意脚本执行，并为可能挂起的操作设置明确超时。

### 1.5 Windows 路径和文件系统规则

Windows 还需要处理盘符、反斜杠、保留名称、NTFS Alternate Data Streams、junction、符号链接、大小写不敏感和安全软件临时锁文件等问题。

当前实现已经检查绝对路径、父目录穿越、Windows 保留名称、ADS、符号链接越界和产物目录边界，并默认不覆盖已有文件。仍需保留 Windows 实机测试。

## 2. 换到其他电脑也可能出现的问题

### 2.1 重复标签页和错误录像轨道

实机 Edge 扩展会话中曾同时存在两个相同的 GitHub 标签页。录像为不同页面生成了轨道，其中一个页面的截图层返回全白。如果选择错误轨道，最终视频会静止或全白。

录像前必须结合页面列表、URL、当前页面状态和截图确认目标标签页；录像后应抽取多个时间点的关键帧，确认画面确实变化。

### 2.2 页面和元素引用失效

页面切换、重新加载、DOM 更新或重新发现元素后，旧引用会失效，继续点击可能超时。开始录像还会创建新的浏览器上下文，使录像前的页面句柄失效。

页面或上下文变化后，应重新调用 `listPages`，在当前标签页重新执行 `snapshot` 或 `find`，并只使用最新返回的页面和元素引用。

### 2.3 iframe 元素引用格式不一致

普通元素引用形如 `e12`，扩展 iframe 中的引用可能形如 `f2e5`。此前烟测记录称 Host 已兼容两种严格格式，但当前工作区代码仍只接受 `eN`/`@eN`。记录与实现状态不一致，因此该项目前按“已发现、待修复并复测”处理。

### 2.4 FFmpeg 或录像组件缺失

Playwright 的 FFmpeg 组件最初没有安装，导致录像无法正常生成；安装后健康检查才通过。换机时还可能遇到 `ffmpeg` 不在 `PATH`、版本或编解码器不同，以及只安装浏览器而未安装录像组件。

应把“浏览器操作可用”和“录像可用”拆成两个健康状态，避免 FFmpeg 缺失阻断截图和普通浏览器操作。

### 2.5 登录状态和 Profile 不能可靠跨机复制

Cookie、密码、令牌、Passkey、WebAuthn、系统证书和企业身份状态可能与用户、操作系统或浏览器加密密钥绑定。不能假设复制 Profile 后可在另一台电脑继续登录。

每台电脑应使用独立专用 Profile，首次运行允许用户在可见浏览器中登录。MFA、CAPTCHA、Passkey 和身份提供方授权仍由用户完成；Profile 和认证状态不得提交到仓库。

### 2.6 浏览器、驱动和 CLI 版本差异

Edge/Chromium、`agent-browser`、Playwright 浏览器和 FFmpeg 版本差异，可能改变页面快照、引用格式、可执行文件名称和安装位置。应记录验证过的版本组合，在启动时输出版本信息，并在升级后重跑截图、录像、下载和引用测试。

### 2.7 网络和 Host 白名单差异

代理、VPN、DNS、企业证书、防火墙，以及网站跳转到登录域名、CDN 或下载域名，都可能造成换机失败。不能为兼容跳转直接开放任意域名，应根据实际跳转链增加最小必要白名单，并继续由 Host 执行策略检查。

### 2.8 页面布局和账户状态差异

分辨率、DPI、浏览器语言、账户权限、A/B 测试、网站更新以及 headed/headless 差异都可能改变 DOM。自动化不应依赖固定坐标或长期保存的元素引用，而应根据当前页面语义重新发现目标。

### 2.9 性能差异和超时

较慢电脑可能在浏览器启动、页面加载、下载、录像停止或安全软件扫描产物时超过默认超时。启动、页面等待、下载和录像停止应使用不同超时，并在错误中说明具体阶段，避免通过无限延长统一超时掩盖真正的挂起。

### 2.10 产物存在不代表产物有效

本次录像验收检查了 WebM 文件头、多个时间点的关键帧和实际页面变化；ZIP 验收检查了条目数量和根目录。后续仍应遵守：文件存在或大小非零不代表内容正确，不完整产物不能作为成功结果返回。

## 3. 市面上的浏览器自动化工具如何降低这些问题

主流工具并不是靠一个“更强的浏览器插件”解决全部问题，而是把浏览器生命周期、登录态、页面定位、等待重试、安全策略和产物验收拆开处理。常见做法如下。

### 3.1 使用独立浏览器上下文或专用 Profile

Playwright、Puppeteer、Chrome DevTools MCP 和 `agent-browser` 通常默认启动自己管理的 Chromium/Chrome，并使用独立 Browser Context 或专用用户数据目录。这样可以避免与用户日常浏览器争抢 Profile 锁，也可减少扩展、标签页和账户状态相互干扰。

需要长期保持登录时，工具会使用项目专属持久 Profile，或保存 cookies 与 localStorage 后在下一次启动恢复。需要并行任务时，则为每个任务创建独立 context/session，而不是让多个 agent 共同争用同一个活动标签页。

### 3.2 使用语义定位，并在每次动作前重新解析元素

成熟工具优先使用 role、accessible name、label、text、test id 等语义定位器。以 [Playwright Locators](https://playwright.dev/docs/locators) 为例，定位器在每次动作执行时都会针对当前 DOM 重新解析目标，因此 React 重渲染后不必继续使用旧的 DOM 句柄。

`agent-browser` 的 `snapshot` 引用适合压缩页面信息供 agent 使用，但引用仍属于当前页面快照。页面导航、刷新、上下文切换或明显重渲染后，应重新 `snapshot`/`find`，不能把 `@eN` 或 iframe 引用当作长期稳定 ID。

### 3.3 在动作层做可操作性检查、自动等待和有限重试

[Playwright Auto-waiting](https://playwright.dev/docs/actionability) 会在点击前检查元素是否唯一、可见、稳定、可接收事件并已启用。市场上的工具普遍还会等待导航、URL、文本、网络或下载事件，并只对可恢复错误做有限重试。

这能降低页面尚未加载、动画遮挡和按钮暂不可点击造成的失败，但不能解决错误标签页、失效会话、浏览器桥接挂起或登录过期。因此超时必须分阶段，并保留可诊断错误，而不是无限重试。

### 3.4 优先使用 CDP/WebDriver 直接控制，扩展桥接只用于特殊场景

Playwright、Puppeteer、Selenium 和 `agent-browser` 的常规路径是由工具启动浏览器，再通过 CDP 或 WebDriver 控制。这样浏览器版本、Profile、进程和连接端点都由工具掌握，通常比依赖浏览器扩展、扩展后台页和本地消息桥更容易诊断。

浏览器扩展仍有价值：它可以让用户明确授权 agent 接管一个已经打开、已经登录的日常浏览器，也可能绕过某些网站对自动化启动浏览器的登录限制。但扩展多了一层版本、权限、后台生命周期和连接状态，不能天然解决重复标签页、错误页面选择或 Profile 锁等问题。

Chrome 官方的 [Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp) 也同时提供两种模式：默认启动专用 Profile；需要复用人工浏览状态时，连接正在运行且允许远程调试的 Chrome。这说明“独立受管浏览器”与“接管已登录浏览器”本来就是两个需要明确选择的运行模式。

### 3.5 显式管理页面所有权和会话生命周期

工具会为 browser/context/page/session 分配句柄，列出当前页面，并在动作前确认目标 URL 或标题。更稳妥的 agent 工作流还会要求：一个任务拥有一个明确会话；新开页面后重新选页；录制开始后重新获取新 context 内的页面；关闭页面时只关闭任务自己创建的页面。

本项目应继续把 request/response 操作与录像、事件等 streaming subscription 分开，不能通过一个隐式“当前页面”承载所有状态。

### 3.6 把登录态当作密钥材料管理

[Playwright Authentication](https://playwright.dev/docs/auth) 明确提醒认证状态文件可能包含可冒用账户的 cookies 和 headers。业界通常将认证状态写到不入库的目录，限制文件权限，按账户和环境隔离，定期失效，并在 MFA、CAPTCHA、Passkey 或高风险授权时让用户人工完成。

跨机器时，工具一般不会承诺直接复制整个日常 Profile。更可靠的做法是在新机器的专用 Profile 中重新登录，或由受信任的部署流程注入短期认证状态。涉及系统密钥链、设备绑定或 Passkey 时，仍必须重新授权。

### 3.7 启动预检、版本固定和真实产物验收

成熟方案会在启动前检查浏览器、驱动、原生可执行文件、端口和录制组件，固定经过验证的版本组合，并在升级后重跑冒烟测试。`agent-browser doctor` 可作为依赖和安装状态的第一层检查；截图、视频和下载结果还需验证内容，而不只检查文件是否存在。

### 3.8 对本项目的适配结论

当前最合适的主路径不是再增加一套浏览器插件，而是继续使用单一 Capability Host，通过 `agent-browser` 直接管理项目专属持久 Profile。用户第一次在 headed 浏览器中完成登录，后续由相同 Profile 复用登录态。这与当前 `examples/windows-browser/bindings.windows.json` 中的 `profileDirectory` 设计一致。

连接用户已经打开的浏览器适合一次性协作、排障或导入登录态，但不应成为默认生产路径。无论底层用 CDP、扩展还是状态文件，Code Mode 看到工具也不代表得到授权；每次调用仍必须经过 Host policy，且浏览器允许访问的域名、页面和本地文件范围不能超过 Host 授权。

## 4. `agent-browser` 如何操作已经登录的页面

根据 [`agent-browser` 官方 README](https://github.com/vercel-labs/agent-browser/blob/main/README.md)，目前有四条实用路径。

### 4.1 项目专属持久 Profile：推荐的正式路径

使用一个专门目录作为 `--profile <path>`。第一次以 headed 模式打开浏览器，由用户人工登录；以后继续使用同一路径，cookies、localStorage、IndexedDB、service workers、缓存和登录会话都会保留。

本项目已经实现这条路径：当未配置 `cdpEndpoint` 和 `autoConnect` 时，Host 会把受控的 `profileDirectory` 作为 `--profile` 参数传给原生 `agent-browser` 可执行文件。示例配置使用 `artifacts/browser-profile`。

优点是隔离、可复现、容易确定页面所有权，也不会直接暴露用户所有日常标签页。缺点是需要在每台新电脑上首次登录，部分网站仍会要求重新 MFA 或设备验证。

### 4.2 读取 Chrome 已有 Profile 的只读快照：适合一次性迁移

官方 CLI 支持 `agent-browser profiles` 列出 Chrome Profile，并用 `--profile Default` 或显示名称复制一份临时只读快照。它不会修改原 Profile；在 Windows 上，Chrome 运行时可能锁定文件，因此官方要求必要时先关闭 Chrome。

当前 Host 会把 `profileDirectory` 规范化为一个受控目录路径，所以“按 Profile 名称复制日常 Chrome Profile”并不是当前配置直接开放的能力。如需加入，应新增明确的受信任配置类型和策略检查，不能把任意用户目录路径直接交给 Code Mode。

### 4.3 通过 CDP 接管正在运行、已经登录的 Chrome：适合即时人工协作

Chrome 开启远程调试后，`agent-browser` 可以通过 `--cdp <port|url>` 精确连接，也可通过 `--auto-connect` 自动发现。连接成功后，它可以列出标签页、切换到目标页、重新生成 snapshot，并执行 click/fill/press/read 等正常操作；操作发生在该 Chrome Profile 的现有登录态中。

本项目 Host 已有对应配置：`cdpEndpoint` 优先于 `autoConnect`，配置其一后不再启动项目 Profile。对于可重复测试，优先使用明确的 loopback CDP endpoint；`autoConnect` 更方便，但在多浏览器、多 Profile 或多调试端口环境中更容易连错实例。

此路径权限很大：官方说明远程调试端口会向本机进程暴露完整浏览器控制；Chrome DevTools MCP 也提醒，连接后可访问所选 Profile 的全部窗口。因此只能在可信机器上临时启用，端点应绑定 loopback，用完关闭，并在 Host 层过滤未授权页面。用户的邮箱、支付和管理后台等敏感标签页不应与被接管会话同时开放。

### 4.4 从已登录浏览器导出认证状态，再在隔离会话中加载

官方推荐的导入流程是先连接已经登录的 Chrome，执行 `state save` 保存认证状态，后续通过 `--state <path>` 加载；也可用稳定 `--session <id> --restore` 自动保存和恢复 cookies 与 localStorage。这比长期接管日常浏览器更隔离，也比复制完整 Profile 更轻量。

状态 JSON 含有明文 session token，必须放在忽略提交且权限受限的位置，按秘密文件管理；需要静态加密时可配置 `AGENT_BROWSER_ENCRYPTION_KEY`。状态文件不能跨站点保证完整恢复 IndexedDB、设备绑定认证、Passkey 或系统证书。

当前 Host 尚未把 `state save/load` 或 `--restore` 作为受控 operation 暴露。因此项目现在能使用的是“持久 Profile”与“CDP/auto-connect”两条路径；若以后增加状态导入，应由 Host 固定输入输出目录、禁止把 token 内容返回给 Code Mode、继续执行域名和动作策略，并加入版本回归测试。官方 changelog 曾修复原生 Rust 重写后 `--state` 参数实际未加载的问题。本机当前安装的是 `agent-browser 0.33.2`，已晚于该历史修复；但正式启用状态导入时仍应固定版本并做 Host 端集成回归测试，不能只凭 CLI 版本判断功能可用。

### 4.5 推荐工作流

正式使用采用：项目专属 Profile → 用户首次 headed 登录 → Host `attach/listPages` → 校验目标 URL → 重新 `snapshot/find` → 执行动作。每台电脑使用自己的 Profile，不复制到仓库。

临时使用已有登录页采用：用户显式开启 Chrome 远程调试 → Host 使用明确 `cdpEndpoint` 连接 → `listPages` 后按 URL 选定页面 → 操作前重新 snapshot → 完成后断开并关闭远程调试。不要默认接管用户日常浏览器，也不要让 Code Mode 自行选择任意 CDP 地址。

如需把已有登录态迁入隔离环境，后续可新增 Host 受控的“导入认证状态”管理操作；它应是安装/配置阶段的可信操作，而不是开放任意 CLI 参数或第二套工具传输。

## 5. 当前结论与待补充项

当前证据表明，原生 JavaScript Code Mode 的协议和工具投影链路没有暴露出功能性故障。问题主要集中在 Host 进程能否启动、浏览器驱动与扩展桥接、页面会话生命周期，以及本机依赖和安全策略。

正式关闭本清单前至少还需：

- 修复并测试 `fNeM` iframe 引用，或明确取消对该扩展路径的支持。
- 验证非 nvm-windows 安装布局与 Windows ARM64。
- 处理异常退出后遗留的 `.agent-browser-call_*` 临时文件。
- 加入 Windows Application Control 的预检与部署故障排查。
- 追加下一轮实机运行结果和新发现。
