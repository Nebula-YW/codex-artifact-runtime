# Windows 端 Codex 通过 Dynamic Tools 控制已登录浏览器并采集社交媒体评论

## 1. 背景

希望在 Windows 环境中运行 Codex CLI，并通过 Code Mode 执行自动化任务。

任务需要操作用户日常使用、已经登录社交媒体账号的浏览器，进入指定页面或帖子，读取评论区内容，并将评论整理后写入本地文件。

整个过程需要尽量屏蔽 Windows、macOS 和 Linux 之间的 Shell、路径及命令转义差异，为后续跨平台运行提供统一的工具接口。

---

## 2. 核心目标

实现以下完整流程：

```text
用户提出任务
    ↓
Codex Code Mode 编排任务
    ↓
连接用户已经登录的浏览器
    ↓
打开指定社交媒体页面
    ↓
进入帖子或内容详情页
    ↓
读取评论区
    ↓
提取并清洗评论
    ↓
写入本地 Markdown 或 JSON 文件
```

示例任务：

```text
打开我已登录的浏览器，进入指定社交媒体帖子，
读取当前评论区的留言，并将评论保存到：

workspace/comments/social-comments.md
```

---

## 3. 总体设计原则

### 3.1 不依赖 Shell 命令字符串

Codex 不应通过以下方式完成任务：

```text
powershell -Command "..."
cmd /c "..."
bash -c "..."
agent-browser open ... && agent-browser snapshot ...
echo "..." > comments.md
```

应通过 Codex Dynamic Tools 的结构化调用完成：

```ts
await tools.browser.navigate({
  session_id: "browser-session",
  url: targetUrl,
});

await tools.fs.writeText({
  root: "workspace",
  path: "comments/social-comments.md",
  content: markdown,
});
```

目标是避免：

* PowerShell 转义规则
* cmd 转义规则
* Bash 与 zsh 语法差异
* 管道和重定向差异
* 环境变量写法差异
* Windows 路径反斜杠问题

---

### 3.2 Code Mode 只负责编排

Code Mode 负责：

* 理解用户意图
* 决定操作步骤
* 调用结构化工具
* 处理工具返回的数据
* 清洗和组织评论内容
* 生成最终文件内容

Code Mode 不负责：

* 直接管理浏览器进程
* 长期保存浏览器对象
* 解析操作系统路径
* 拼接 Shell 命令
* 直接访问任意本地目录

---

### 3.3 浏览器状态由 Browser Host 保存

浏览器属于有状态应用。

因此，用户的浏览器会话应由长期运行的 Browser Host 进程管理。Browser Host 通过 Host 能力实现接入 Dynamic Tools，不引入第二套工具传输协议。

Browser Host 负责保存：

```text
browser_session
browser_context
page / tab
cookie
localStorage
登录状态
当前 URL
页面事件
下载任务
弹窗
```

Codex 只持有不透明引用：

```text
session_id
page_id
artifact_id
```

例如：

```ts
await tools.browser.click({
  page_id: "page-01",
  locator: {
    role: "button",
    name: "查看评论",
  },
});
```

---

### 3.4 文件使用逻辑路径

Code Mode 不应看到真实操作系统路径：

```text
C:\Users\xin\projects\engraph\comments\data.md
/Users/xin/projects/engraph/comments/data.md
/home/xin/projects/engraph/comments/data.md
```

统一使用：

```ts
{
  root: "workspace",
  path: "comments/data.md"
}
```

其中：

* `root` 表示预先授权的目录
* `path` 表示相对于 root 的逻辑路径
* 逻辑路径统一使用 `/`
* 不允许绝对路径
* 不允许 `..` 跨目录访问
* 不依赖当前工作目录 `cwd`

Host 内部负责将逻辑路径转换为 Windows 原生路径。

---

## 4. 总体架构

```text
┌────────────────────────────────────────────┐
│ Windows                                    │
│                                            │
│  ┌─────────────────────┐                   │
│  │ Codex CLI           │                   │
│  │ Code Mode           │                   │
│  └──────────┬──────────┘                   │
│             │                              │
│             │ Codex Dynamic Tools          │
│             ▼                              │
│  ┌─────────────────────┐                   │
│  │ Capability Host     │                   │
│  │ Policy + Dispatch   │                   │
│  │                     │                   │
│  │ browser.*           │                   │
│  │ fs.*                │                   │
│  │ approval.*          │                   │
│  └───────┬────────┬────┘                   │
│          │        │                        │
│          │        └──────────────┐         │
│          ▼                       ▼         │
│  ┌─────────────────┐    ┌────────────────┐ │
│  │ Browser Host    │    │ File Host      │ │
│  │ Session Daemon  │    │ Path Sandbox   │ │
│  └────────┬────────┘    └───────┬────────┘ │
│           │                     │          │
│           ▼                     ▼          │
│  ┌─────────────────┐    ┌────────────────┐ │
│  │ 已登录浏览器    │    │ 本地 Workspace│ │
│  │ Chrome / Edge   │    │ comments/*.md │ │
│  └─────────────────┘    └────────────────┘ │
└────────────────────────────────────────────┘
```

---

## 5. 浏览器使用方式

## 5.1 使用用户已经登录的浏览器

系统需要支持连接用户日常使用的浏览器，而不要求用户在自动化浏览器中重新登录。

期望复用：

* 用户已有 Cookie
* 已登录账号
* 已打开标签页
* LocalStorage
* 浏览器 Profile
* 用户授权状态

可能的实现方式包括：

* 连接已开启调试端口的 Chrome 或 Edge
* 通过浏览器扩展建立连接
* 通过 CDP 连接指定浏览器实例
* 通过 Browser Host 附着到已有浏览器
* 使用专门的用户 Profile 启动受控浏览器

Code Mode 不直接关心使用的是哪种连接方式。

统一接口类似：

```ts
const session = await tools.browser.attach({
  target: "user-browser",
  profile: "default",
});
```

---

## 5.2 浏览器工具接口

浏览器能力应由统一的 Capability Catalog 描述，并投影为 Codex App Server Dynamic Tools。Code Mode 通过全局 `tools` 对象调用这些能力。

建议提供以下基础接口：

```ts
interface BrowserDynamicTools {
  attach(input: {
    target: string;
    profile?: string;
  }): Promise<BrowserSession>;

  listPages(input: {
    session_id: string;
  }): Promise<PageSummary[]>;

  openPage(input: {
    session_id: string;
    url: string;
  }): Promise<PageRef>;

  navigate(input: {
    page_id: string;
    url: string;
  }): Promise<void>;

  snapshot(input: {
    page_id: string;
  }): Promise<PageSnapshot>;

  click(input: {
    page_id: string;
    locator: Locator;
  }): Promise<ActionResult>;

  read(input: {
    page_id: string;
    locator: Locator;
  }): Promise<ReadResult>;

  scroll(input: {
    page_id: string;
    direction: "up" | "down";
    amount?: number;
  }): Promise<void>;

  waitFor(input: {
    page_id: string;
    condition: WaitCondition;
  }): Promise<void>;

  closePage(input: {
    page_id: string;
  }): Promise<void>;
}

declare const tools: {
  browser: BrowserDynamicTools;
};
```

---

## 5.3 元素定位策略

优先使用语义定位，不依赖屏幕坐标。

推荐优先级：

```text
role + accessible name
data-testid
label
placeholder
稳定文本
CSS selector
XPath
屏幕坐标
```

例如：

```ts
await tools.browser.click({
  page_id,
  locator: {
    role: "button",
    name: "展开更多评论",
  },
});
```

不建议长期保存 DOM Element Handle。

页面刷新、局部渲染或导航后，应重新通过 Locator 查找元素。

---

## 6. 社交媒体浏览流程

典型流程如下。

### 6.1 附着浏览器

```ts
const session = await tools.browser.attach({
  target: "user-browser",
});
```

### 6.2 获取已有标签页

```ts
const pages = await tools.browser.listPages({
  session_id: session.id,
});
```

系统可以：

* 使用用户已经打开的社交媒体标签页
* 打开一个新标签页
* 导航到用户指定 URL

### 6.3 打开目标帖子

```ts
const page = await tools.browser.openPage({
  session_id: session.id,
  url: targetUrl,
});
```

### 6.4 等待页面完成加载

```ts
await tools.browser.waitFor({
  page_id: page.id,
  condition: {
    type: "element",
    locator: {
      role: "region",
      name: "评论",
    },
  },
});
```

### 6.5 展开评论区

可能需要执行：

```text
点击“查看全部评论”
点击“展开回复”
点击“加载更多”
向下滚动
等待异步评论加载
```

### 6.6 读取评论

每条评论尽量提取：

```text
评论正文
作者名称
发布时间
评论层级
父评论
点赞数量
回复数量
来源帖子
抓取时间
```

其中，最基本的必需字段是评论正文。

---

## 7. 评论数据结构

建议内部使用结构化数据：

```ts
interface SocialComment {
  id?: string;
  text: string;
  author?: string;
  published_at?: string;
  parent_id?: string;
  like_count?: number;
  reply_count?: number;
  source_url: string;
  captured_at: string;
}
```

示例：

```json
{
  "text": "这个观点很有意思",
  "author": "用户 A",
  "published_at": "2026-08-05T10:12:00+08:00",
  "source_url": "https://social.example/post/123",
  "captured_at": "2026-08-05T11:30:00+08:00"
}
```

---

## 8. 评论清洗要求

评论提取完成后，需要进行基础清洗。

包括：

* 去除按钮文字
* 去除“点赞”“回复”等 UI 文本
* 合并换行
* 去除空评论
* 去除完全重复的评论
* 保留 Emoji
* 保留原始语言
* 可选过滤广告和机器人内容
* 可选过滤只有表情或只有链接的评论
* 保留原始抓取顺序

需要避免将以下内容误识别为评论：

```text
写下你的评论
查看更多
展开回复
点赞
回复
分享
举报
```

---

## 9. 本地文件写入

## 9.1 文件能力

文件操作通过 Dynamic Tools 中的 `tools.fs.*` 能力完成。

例如：

```ts
await tools.fs.writeText({
  root: "workspace",
  path: "comments/social-comments.md",
  content: markdown,
  create_parents: true,
});
```

禁止使用：

```text
echo "..." > file.md
Out-File
Set-Content
cat > file
```

---

## 9.2 文件路径规范

统一使用：

```text
root + 相对路径
```

例如：

```json
{
  "root": "workspace",
  "path": "comments/social-comments-2026-08-05.md"
}
```

Host 需要拒绝：

```text
C:\Windows\System32\...
\\server\share\...
..\..\secret.txt
/Users/...
/etc/passwd
file.txt:stream
```

Host 还需要处理：

* Windows 保留名称
* 大小写冲突
* 尾部空格
* 尾部点号
* 非法字符
* 符号链接逃逸
* 原子写入
* 并发修改冲突

---

## 9.3 Markdown 输出格式

推荐默认生成 Markdown：

```md
# 社交媒体评论记录

- 来源：<帖子 URL>
- 抓取时间：2026-08-05 11:30
- 评论数量：3

## 评论

### 1. 用户 A

- 时间：2026-08-05 10:12
- 内容：

这个观点很有意思。

### 2. 用户 B

- 时间：2026-08-05 10:18
- 内容：

已收藏，感谢分享。

### 3. 用户 C

- 时间：2026-08-05 10:35
- 内容：

这里说得不太对，建议补充来源。
```

---

## 9.4 JSON 输出格式

对于后续分析，也可以同时输出 JSON：

```json
{
  "source_url": "https://social.example/post/123",
  "captured_at": "2026-08-05T11:30:00+08:00",
  "comments": [
    {
      "text": "这个观点很有意思",
      "author": "用户 A"
    }
  ]
}
```

推荐：

```text
comments/post-123.md
comments/post-123.json
```

Markdown 用于人工阅读，JSON 用于程序处理。

---

## 10. Codex Dynamic Tools 与 Code Mode

系统只使用 Codex 原生 JavaScript Code Mode。浏览器、文件和审批能力由同一个 Capability Catalog 描述，再投影为 Codex App Server 的 `dynamicTools`：

```text
Capability Catalog
  ↓
Codex App Server dynamicTools
  ↓
原生 Code Mode JavaScript
  ├─ tools.browser.*
  ├─ tools.fs.*
  └─ tools.approval.*
       ↓
Capability Host policy seam
  ├─ 输入校验
  ├─ 授权与审批
  ├─ Host 调度
  └─ 输出校验
```

Code Mode 直接编排 Dynamic Tools：

```ts
await tools.browser.navigate({
  page_id,
  url,
});
```

工具可见性只用于能力发现，不代表调用已经获得授权。每次调用都必须经过 Capability Host 的策略边界；Host 根据站点范围、操作风险、用户授权和当前会话状态决定允许、拒绝或请求审批。

`dynamicTools` 只投影请求/响应操作。页面事件、下载进度等流式订阅保留在独立的 Host 通道中，不伪装成 Dynamic Tool 调用。

该链路不增加额外的 Code Mode 运行时，也不增加第二套工具传输。

---

## 11. 权限与安全要求

控制用户真实浏览器属于高权限能力。

系统需要至少提供以下约束。

### 11.1 站点范围限制

只允许访问用户授权的域名：

```text
social.example.com
www.example.com
```

默认禁止访问：

* 网上银行
* 支付页面
* 密码管理器
* 企业后台
* 云服务控制台
* 本地路由器管理页面
* 浏览器内部页面

---

### 11.2 操作分级

#### 只读操作

可以自动执行：

```text
打开页面
读取帖子
读取评论
滚动页面
展开评论
保存本地文件
```

#### 低风险写操作

可以根据用户配置决定是否确认：

```text
点赞
收藏
关注
填写草稿
下载公开文件
```

#### 高风险操作

必须人工确认：

```text
发表评论
发送私信
删除内容
修改账号资料
上传文件
付款
购买
修改密码
授权第三方应用
```

---

### 11.3 浏览器数据隔离

Browser Host 不应默认将以下信息返回给模型：

* 完整 Cookie
* 登录 Token
* 浏览器密码
* Authorization Header
* Session Storage 中的敏感信息
* 信用卡信息
* 自动填充数据

Codex 只需要知道浏览器当前是否已经登录，不需要读取登录凭据。

---

## 12. 状态管理与恢复

### 12.1 实时状态

Browser Host 内存中保存：

```text
session_id
page_id
当前 URL
标签页列表
浏览器连接
事件监听器
```

### 12.2 可恢复状态

可以保存：

```text
session 配置
目标 URL
已完成步骤
已抓取评论 ID
输出文件路径
任务检查点
```

例如：

```json
{
  "task_id": "task-001",
  "session_id": "browser-session",
  "source_url": "https://social.example/post/123",
  "checkpoint": "comments_page_3",
  "output": {
    "root": "workspace",
    "path": "comments/post-123.md"
  }
}
```

### 12.3 重复执行

重复运行时，需要避免将同一评论多次写入文件。

可以依据以下字段去重：

```text
平台评论 ID
作者 + 时间 + 评论正文
评论正文哈希
```

---

## 13. 错误处理

系统需要处理以下异常：

### 浏览器连接失败

```text
未找到可连接的浏览器
浏览器调试接口未开启
浏览器版本不兼容
浏览器被用户关闭
```

### 登录状态失效

```text
账号退出
Cookie 过期
页面要求重新验证
出现验证码
```

此时应暂停任务，并提示用户完成登录或验证。

### 页面结构变化

```text
评论按钮找不到
评论区改版
元素定位失败
页面异步加载超时
```

应优先重新获取页面 Snapshot，并使用新的语义 Locator 定位。

### 评论加载不完整

可能原因：

```text
无限滚动
评论分页
需要反复点击“加载更多”
平台限制未登录用户查看数量
评论被折叠
网络超时
```

最终结果中应标记：

```text
是否完整
加载了多少页
是否因为限制提前停止
```

### 本地文件写入失败

可能原因：

```text
目录没有权限
目标文件正在被占用
磁盘空间不足
文件发生并发修改
路径不合法
```

写入失败时不能直接覆盖其他版本。

---

## 14. 非目标

当前阶段暂不要求：

* 绕过验证码
* 绕过平台访问限制
* 绕过登录验证
* 大规模批量抓取整个社交平台
* 自动发表评论
* 自动发送私信
* 自动进行营销互动
* 使用用户账号执行高风险操作
* 通过屏幕坐标作为主要控制方式
* 在 Code Mode 中直接启动和管理浏览器后台进程

---

## 15. 第一阶段建议范围

第一阶段仅实现只读评论采集：

```text
Windows 原生运行 Codex
Code Mode 编排
附着到用户已登录 Chrome 或 Edge
访问用户指定帖子 URL
展开可见评论
提取评论正文
可选提取作者和时间
去重
写入 workspace 下的 Markdown 文件
```

第一阶段不执行：

```text
点赞
收藏
关注
发帖
评论
私信
删除
付款
修改账号
```

---

## 16. 验收标准

满足以下条件即可认为第一阶段完成。

### 跨平台调用

* Code Mode 中不存在 PowerShell 命令拼接
* 不通过 Shell 重定向写文件
* 浏览器调用使用结构化参数
* 文件路径使用 `root + 相对路径`

### 浏览器

* 可以连接 Windows 上用户已登录的浏览器
* 可以识别当前标签页
* 可以打开用户指定的帖子
* 可以进入评论区
* 可以展开至少一层分页或“加载更多”
* 可以读取页面中实际可见的评论

### 数据

* 可以提取评论正文
* 可以过滤明显的 UI 文本
* 可以对重复评论去重
* 可以记录来源 URL
* 可以记录采集时间

### 文件

* 可以写入 workspace 内指定 Markdown 文件
* 自动创建必要的父目录
* 不允许访问 workspace 外部路径
* 写入失败时返回结构化错误

### 安全

* 默认只允许读取页面
* 不读取 Cookie 和登录 Token
* 不自动发布、删除或付款
* 高风险操作需要人工审批
* 可以配置允许访问的站点范围

---

## 17. 最终目标架构

```text
用户
  ↓
Codex CLI / App Server
  ↓
原生 JavaScript Code Mode
  ↓
Dynamic Tools（由 Capability Catalog 生成）
  ├─ tools.browser.*
  ├─ tools.fs.*
  └─ tools.approval.*
       ↓
Capability Host policy seam
  ↓
Windows Host
  ├─ Browser Host
  │    └─ 用户已登录的 Chrome / Edge
  │
  └─ File Host
       └─ workspace/comments/*.md
```

核心边界是：

> Code Mode 保存任务意图和编排逻辑，Dynamic Tools 提供结构化能力入口，Capability Host 对每次调用执行策略校验，Browser Host 保存浏览器活状态，File Host 负责路径解析、权限控制和安全写入。

通过这种方式，可以在 Windows 上操作用户已登录的浏览器，同时尽量屏蔽 PowerShell、cmd、路径分隔符和文件系统语义带来的跨平台差异。
