# Inkdown

[![build](https://github.com/ice-a/inkdown/actions/workflows/build.yml/badge.svg)](https://github.com/ice-a/inkdown/actions/workflows/build.yml)

用 Rust + [GPUI Kit](https://gpui-kit.com/zh-CN/) 编写的 Markdown 写作与分发工具。
左边是文字，右边是它长成的样子；写完可以排版、上传图片、发布到公众号 / 知乎 / 头条 / CSDN / 博客园等平台，
也能把文稿放进本地文件夹、做 zip 备份并同步到 Git。

![Inkdown 截图](docs/screenshot-light.png)

## 功能

- **编辑器**：Markdown 语法高亮、行号、软换行、内置查找、撤销/重做、格式化工具栏与快捷键。
- **首屏工作台**：没有内容时直接给出“从模板开始 / 空白文档 / 打开文件 / 打开文件夹 / AI 写作 / 载入示例”，不用先翻菜单。
- **斜杠快捷指令**：编辑器里输入 `/` 打开命令面板，可插入表格、图片、时间、代码块，询问 AI，或运行自己写的 JavaScript / Python 命令。
- **模板库**：普通文章、技术笔记、周报、会议纪要，自带可直接修改的 frontmatter 和正文骨架（`Ctrl + Shift + N`）。
- **最近文稿**：打开过的文件会出现在侧栏“最近”，退出后重新启动会恢复上次编辑的文件。
- **粘贴图片**：截图后直接 `Ctrl + V`，图片自动存到文档旁的 `assets/`（BMP 自动转 PNG）
  并插入 `![](assets/…)`；图床配置好后可自动上传并插入公开链接。
- **预览查找**：`Ctrl + F` 在预览中高亮所有匹配并逐条跳转（编辑器内 `Ctrl + F` 仍是编辑器查找）。
- **文档元数据**：`Ctrl + Shift + M` 打开 frontmatter 字段编辑面板，可视化增删改。
- **拖拽打开**：把 `.md` 文件或文件夹拖进窗口即可打开。
- **实时预览**：Markdown 渲染、表格、任务列表、代码块、引用、本地图片；可缩放。代码块带
  **多语言语法高亮**（约 200 种语法定义），右上角的语言按钮可以直接切换语言。
- **YAML frontmatter**：文章开头的 `---` 元数据块渲染为信息卡片（title/author/url…），
  不会变成两条分隔线；统计与发布时自动排除元数据。
- **主题**：暖纸浅色 / 墨夜深色，主题选择会记住。
- **文稿管理**：最近文稿、标签页式文稿列表、目录大纲（点击跳转编辑器并定位预览）。
- **本地文件夹**：打开文件夹、文件树、新建 / 重命名 / 删除文件与文件夹、刷新、在文件管理器中显示；空文件夹直接提供新建入口。
- **图床**：CloudFlare-ImgBed、Lsky Pro；发布前自动上传本地图片并改写 Markdown 源码。
- **AI 写作**：任何 OpenAI 兼容接口（OpenAI、DeepSeek、Moonshot、Ollama、LM Studio、自建网关），
  可配置 Base URL / API Key / 提示词，一键拉取模型列表选择，生成新文档或插入当前光标处。
- **AI 对话**：工具栏“询问 AI”或 `/询问ai` 打开对话窗口，多轮上下文、可参考当前文稿，
  回复一键插入光标处；与 AI 写作共用设置里的接口配置。
- **发布**：公众号草稿 API、博客园 MetaWeblog、知乎/头条/CSDN/简书/掘金“复制富文本 + 打开发布页”。
- **公众号排版**：内联样式 HTML，代码块、表格、引用、任务列表都有专门样式；外链可自动转文末参考列表。
- **备份**：一键把当前文件夹（或未保存文稿）打包成 zip；可设置保存后自动备份。
- **Git**：初始化仓库、提交、拉取、推送、保存后自动同步；使用系统 git 与凭据管理器。
- **应用图标**：Windows EXE / 开始菜单 / MSI、macOS `.app` / Dock、Linux 启动器都使用同一套 Inkdown 图标。
- **安全退出**：有未保存文档时关闭窗口会先弹出确认；配置改动即时自动保存，不会丢。

## 环境要求

- Windows 10/11（本项目主要在 Windows 上开发；GPUI Kit 也支持 macOS/Linux）。
- Rust **1.92+**（MSVC toolchain）。
- Visual Studio 2022 Build Tools / Community，勾选“使用 C++ 的桌面开发”（MSVC + Windows SDK）。
- CMake（GPUI 构建需要）。
- Git（仅 Git 同步功能需要）。
- 能访问 crates.io 的网络（首次编译）。

## 运行

```bash
cargo run
```

首次编译需要下载并编译 GPUI 相关依赖，耗时较长；之后增量编译会快很多。发布版本：

```bash
cargo build --release
./target/release/inkdown
```

Windows 的 debug 和 release 都使用 GUI 子系统，双击不会弹出 cmd 窗口。需要查看
stdout/stderr 调试时，用 `cargo run --features console` 构建控制台版本。

### 安装包

每个 tag（如 `v0.1.5`）会自动构建并发布三平台安装包：

| 平台 | 包 | 安装方式 |
| --- | --- | --- |
| Windows x64 | `.exe`（推荐） | 运行 `Inkdown-*-setup.exe`，按向导选择安装位置、数据目录、配置目录和桌面快捷方式；按用户安装，无需管理员权限 |
| Windows x64 | `.msi` | 适合静默 / 企业部署；双击安装到 `%LOCALAPPDATA%\Programs\Inkdown`，无需管理员权限 |
| macOS（Apple Silicon） | `.dmg` | 内含标准 `Inkdown.app`，拖入 Applications；未做公证签名，首次请右键 → 打开 |
| Linux x64 | `.deb` | `sudo apt install ./Inkdown-*.deb`，启动器图标和菜单项会自动安装 |

Windows 的 `.exe` 与 `.msi` 二选一即可，不要同时安装两种包。已有的 `v0.1.x` MSI
可以直接用新的 MSI 升级；改用 Setup.exe 时建议先在“设置 → 应用”里卸载旧版本。

Setup.exe 的向导会写入 `install.json` 到安装目录：

- **安装位置**：`inkdown.exe` 与卸载程序所在目录；
- **数据存储位置**：zip 备份、没有归属文稿的粘贴图片等生成数据（默认 `%LOCALAPPDATA%\Inkdown\data`）；
- **配置存储位置**：`config.json` 所在目录（默认 `%LOCALAPPDATA%\Inkdown\config`），
  之后可以单独备份、迁移或导入；
- **桌面快捷方式**：默认创建，可在向导最后一页取消。

卸载时只删除程序文件、快捷方式和 `install.json`，**不会删除**数据目录和配置目录，
文稿、备份和密钥都留在原处。

Release 页面：https://github.com/ice-a/inkdown/releases

## 快捷键

| 操作 | 快捷键 |
| --- | --- |
| 新建文档 | `Ctrl + N` |
| 从模板新建 | `Ctrl + Shift + N` |
| 打开 Markdown | `Ctrl + O` |
| 打开文件夹 | `Ctrl + Shift + O` |
| 保存 / 另存为 | `Ctrl + S` / `Ctrl + Shift + S` |
| 导出 HTML | `Ctrl + Shift + E` |
| 发布中心 | `Ctrl + Shift + P` |
| AI 写作 | `Ctrl + Shift + A` |
| 斜杠快捷指令 | 输入 `/`；`↑` / `↓` 选择，`Enter` / `Tab` 执行，`Esc` 关闭 |
| Git 同步 | `Ctrl + Shift + G` |
| 加粗 / 斜体 / 行内代码 / 链接 | `Ctrl + B` / `Ctrl + I` / `Ctrl + E` / `Ctrl + K` |
| 写作 / 分栏 / 预览视图 | `Ctrl + 1` / `Ctrl + 2` / `Ctrl + 3` |
| 切换主题 / 侧边栏 | `Ctrl + Shift + T` / `Ctrl + Shift + B` |
| 关闭弹层 | `Esc` |

## AI 写作

![AI 生成](docs/screenshot-ai.png)

设置 → **AI 写作**：

| 字段 | 说明 |
| --- | --- |
| Base URL | OpenAI 兼容服务地址，需包含 `/v1`，如 `https://api.openai.com/v1`、`https://api.deepseek.com/v1`、本机 Ollama 的 `http://127.0.0.1:11434/v1` |
| API Key | `Authorization: Bearer` 使用，只保存在本机配置文件 |
| 模型 | 手动填写，或点 **拉取模型** 从 `GET {base}/models` 读取后从下拉中选择 |
| 提示词 | 作为 system 消息发送，决定文章的身份、结构与输出格式 |

用法：菜单栏 **AI 写作** 按钮（或 `Ctrl + Shift + A`）打开弹层 →

1. 填写主题/要求（越具体越好：主题、字数、读者、语气）；
2. 可选打开 **参考当前文档**，把当前文稿（最多 6000 字）作为上下文，用于续写、改写、扩写；
3. **生成新文档** 打开一个以文章标题命名的新文稿；**插入光标处** 把结果拼接进当前文档
   （选中了一段文字则替换选中内容，`Ctrl + Z` 可整体撤销）。

接口走 `POST {base}/chat/completions`（非流式）。生成结果会先显示在弹层日志里，
再去掉模型偶尔包裹的 ```` ```markdown ```` 围栏后写入文稿。

### 询问 AI 对话

工具栏的 **询问 AI** 按钮（或斜杠命令 `/询问ai`）打开一个对话窗口，配置仍然用上面
这一组 Base URL / API Key / 模型，但提示词是对话专用的短提示，而不是生成整篇文章的提示词。

- 多轮对话会带上完整历史，模型能看到前面问过什么；
- `Enter` 发送，`Shift+Enter` 换行；“参考文稿”打开后会把当前文档（最多 6000 字）放进上下文；
- 每条回复下面是 **插入文稿**，点了就把该回复放到光标处并关闭窗口；
- 对话记录只在本次运行内保留，“清空”可以重新开始。

接口与写作弹层相同，仍然是 `POST {base}/chat/completions`（非流式）。

## 斜杠快捷指令

在编辑器里输入 `/`（行首或空白之后）打开命令面板，输入名称实时筛选：
`↑` / `↓` 选择，`Enter` / `Tab` 执行，`Esc` 关闭；也可以直接用鼠标点击。

| 指令 | 作用 |
| --- | --- |
| 插入表格 / 插入图片 | 插入 Markdown 表格；图片优先取剪贴板，否则选择本地文件 |
| 询问 AI | 打开对话窗口，回复可一键插入光标处 |
| 当前时间 | 插入 `YYYY-MM-DD HH:MM` |
| 代码块 | 插入围栏代码块，并打开语言列表选择语言 |
| 引用 / 待办事项 / 插入链接 | 插入对应的 Markdown 片段 |
| 从模板新建 / 文档元数据 | 打开模板库或 frontmatter 编辑面板 |
| 生成命令模板 | 在命令目录生成 JS / Python 示例和说明 |
| 打开命令目录 | 在文件管理器中打开自定义命令目录 |

### 自定义命令

命令目录是**配置目录**下的 `commands/`（面板中的“打开命令目录”可直接打开）。
每个 `.js` / `.mjs` / `.cjs` / `.py` 文件都会出现在面板里，文件名作为默认名称，
文件开头可以写元数据：

```js
// @name 当前时间
// @description 插入当前日期和时间
const chunks = [];
process.stdin.on("data", (chunk) => chunks.push(chunk));
process.stdin.on("end", () => {
  const ctx = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  process.stdout.write(`> ${ctx.now}\n`);
});
```

```python
# @name 文档统计
# @description 统计当前文档字符数并插入一行
import json
import sys

ctx = json.load(sys.stdin)
text = ctx.get("document", "")
print(f"> 当前文档共 {len(text)} 个字符")
```

脚本从 stdin 读取一个 JSON 对象：

```json
{
  "document": "整篇 Markdown",
  "selection": "当前选中的文字",
  "cursor": 123,
  "now": "2026-11-13 11:20:00",
  "date": "2026-11-13",
  "time": "11:20:00",
  "query": "typed-after-slash"
}
```

把要插入到光标位置的 Markdown 写到 stdout；非零退出码会把 stderr 显示在状态栏。
JS 使用 `node`，Python 在 Windows 优先用 `py -3`、否则用 `python`，macOS/Linux 用
`python3` / `python`。Windows 后台执行会隐藏控制台窗口，不会闪 cmd。

> 自定义脚本以当前用户权限运行，可以访问文件和网络。只运行你信任的脚本，
> 不要把来路不明的 `.js` / `.py` 放进命令目录。

## 代码块语言

预览里的围栏代码块会自动做语法高亮。语法集来自 syntect + bat 的语法包（约 200 个语法定义），
`rust`、`python`、`javascript`、`typescript`、`tsx`、`sql`、`yaml`、`bash`、`dockerfile`、
`makefile` 这类常见写法都能直接识别；`c#`、`c++`、`f#`、`objective-c` 这样的符号写法也支持。

每个代码块的右上角有一个语言按钮：

- 点击打开语言列表（60+ 种常用语言，按英文名排序），选中后把语言写进围栏（例如 ```bash），
  预览、编辑器和后续导出立即按该语言着色；
- 没有写语言的围栏显示“语言”，点一下即可补上；
- “纯文本”移除语言标注。

列表里只放语法包确实支持的语言，所以选中就一定会有高亮。列表外的写法（例如 `powershell`，
语法包里没有对应定义）会保持纯文本显示，不会报错；需要高亮时用语言按钮换一个即可。
斜杠命令 `/代码块` 插入围栏后会直接打开语言列表。

## 发布平台说明

| 平台 | 方式 | 需要准备 |
| --- | --- | --- |
| 微信公众号 | 官方草稿箱 API | 认证公众号、AppID/AppSecret、后台 IP 白名单；文章至少有一张图（作为封面），或在设置里填默认封面 |
| 博客园 | MetaWeblog API | 博客园用户名 + MetaWeblog 访问令牌（设置 → 其他设置） |
| 知乎 / 今日头条 / CSDN / 简书 / 掘金 | 富文本复制 + 打开发布页 | 浏览器里已登录对应平台；打开页面后 `Ctrl + V` 粘贴 |
| 任意富文本编辑器 | “只复制排版” | 无 |

为什么不做知乎/头条/CSDN 的全自动提交？这些平台没有公开的文章发布 API，
Cookie/接口逆向方案会随前端改版失效，也可能触发风控。Inkdown 选择稳定、不接触账号密码的方式：
公众号和博客园走官方接口，其余平台把排版好的富文本放进剪贴板并打开发布页。
公众号草稿创建成功后会打开公众号后台，仍需人工确认后群发——这也是微信官方接口的边界。

公众号正文中的外链会被微信移除，因此默认开启“外链转参考”，在文末生成参考链接列表。
封面会优先使用文章第一张图片；没有图片时读取设置里的“默认封面”绝对路径。

## 图床

设置 → 图床：

- **CloudFlare-ImgBed**：填站点根地址，如 `https://img.example.com`；上传认证码填 `authCode`，
  也可以填 API Token（`Authorization: Bearer ...`）。请求 `POST {base}/upload?returnFormat=full`。
- **Lsky Pro**：填站点根地址和 API Token，请求 `POST {base}/api/v1/upload`。

设置里的“粘贴图片自动上传”开启后，编辑器里粘贴的截图会直接上传图床并插入公开链接
（失败时回退为本地 `assets/` 路径）。

发布时勾选“上传本地图片到图床”，Inkdown 会扫描 Markdown 与 HTML 中的本地图片，
上传后把源码里的相对路径替换为图床 URL。公众号发布还会把图片额外上传到微信素材库，
并用第一张图创建封面素材。

## 本地文件夹 / 备份 / Git

- **打开文件夹**：文件 → 打开文件夹，或侧边栏“文件”页签。只展示 Markdown 文件，
  自动跳过 `.git`、`node_modules`、`target` 等目录。
- **备份**：菜单栏的归档按钮，或 文件 → 立即备份。默认写入配置目录下的 `backups`
  （未安装向导时是 `%APPDATA%\Inkdown\backups`，Setup.exe 安装时是所选数据目录的 `backups`），
  可在设置里改目录。备份包含当前文件夹的文稿和资源。
- **Git**：设置里填远端、分支、用户名/邮箱，点击“初始化仓库”后即可“立即同步”。
  同步流程是 `git add -A` → 有改动则 commit → `pull --rebase` → `push -u origin <branch>`。
  用户名/密码交给系统 Git 凭据管理器；Inkdown 设置 `GIT_TERMINAL_PROMPT=0`，不会在后台等待输入密码。
  也可以开启“保存后自动同步”。

## 配置文件

默认配置位置：

```
Windows: %APPDATA%\Inkdown\config.json
macOS:   ~/Library/Application Support/Inkdown/config.json
Linux:   ~/.config/Inkdown/config.json
```

用 Setup.exe 安装时，配置文件会写到向导中选择的**配置存储位置**；程序通过安装目录里的
`install.json` 读取该路径。数据目录（备份等）也可以用向导单独选择，或通过环境变量
`INKDOWN_CONFIG_DIR` / `INKDOWN_DATA_DIR` 覆盖，适合便携版和多套配置。

AppID/AppSecret/Token 等密钥以**明文**保存在本机配置文件中，请勿把该文件提交到公开仓库。
设置面板里的任何修改都会**即时自动保存**到该文件（无需点保存），底部会显示当前文件路径。

## 项目结构

```
build.rs        Windows EXE 图标资源嵌入（resource ID 1）
assets/icon/    图标源图 + PNG / ICO / ICNS
packaging/      WiX MSI 与 NSIS Setup.exe 脚本
src/
  main.rs       应用启动、窗口、快捷键
  app.rs        主视图、编辑器/预览、侧边栏、弹层、动作处理
  actions.rs    所有命令动作
  doc.rs        文档模型、模板、代码块语言列表与围栏定位、大纲、统计、HTML 导出
  commands.rs   斜杠命令：扫描自定义 JS/Python、执行并回传输出
  theme.rs      浅色/深色配色
  ui.rs         设置/发布表单状态
  workspace.rs  文件夹文件树与扫描
  publish.rs    内联样式 HTML、公众号接口、博客园接口、剪贴板
  ai.rs         OpenAI 兼容接口：拉取模型、chat/completions 生成与多轮对话
  imagebed.rs   CloudFlare-ImgBed / Lsky Pro 上传
  backup.rs     zip 备份
  git.rs        系统 git CLI 封装
  config.rs     配置持久化、安装器目录覆盖
  widgets.rs    小部件
```

## 测试

```bash
cargo test
```

当前覆盖：富文本渲染、图片扫描与本地路径解析、图床 multipart 上传（本地 mock HTTP 服务）、
AI 拉取模型 + chat/completions 生成（本地 mock HTTP 服务）、Markdown 围栏清理、
文件树扫描、frontmatter 读写、模板结构、围栏语言定位（含缩进、波浪线和 meta 写法）、
斜杠命令元数据与脚本执行、
zip 备份、Git 初始化/提交/推送到本地裸仓库。

## 已知限制

- 公众号 API 只能创建草稿，最终发布需要在公众号后台确认；个人订阅号没有该接口权限。
- 知乎/头条/CSDN 等平台需要人工粘贴；一次选择多个平台会打开多个标签页。
- 本地图片会原样上传，暂不做压缩或格式转换。
- 首次编译 GPUI 依赖较慢；发布版请使用 `--release`。
