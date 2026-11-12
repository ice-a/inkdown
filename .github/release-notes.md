![Inkdown](https://raw.githubusercontent.com/ice-a/inkdown/main/docs/screenshot-light.png)

**Inkdown** 是用 Rust + [GPUI Kit](https://gpui-kit.com/zh-CN/) 编写的 Markdown 写作与分发工具：
左边写，右边看；写完可以排版、上传图片、发布到公众号 / 知乎 / 头条 / CSDN / 博客园，
也能放进本地文件夹、做 zip 备份、同步到 Git，并用 AI 辅助写作。

## 本次发布包含

- **Windows**：`.msi` 安装包（按用户安装，**无需管理员权限**）
- **macOS（Apple Silicon）**：`.dmg` 磁盘映像
- **Linux x64**：`.deb` 安装包
- 详细变更见下方自动生成的提交列表

## 安装

### Windows

下载 `Inkdown-*-windows-x86_64.msi`，双击安装。程序装到
`%LOCALAPPDATA%\Programs\Inkdown`，开始菜单会出现 **Inkdown**，全程无需 UAC 提权。
卸载：设置 → 应用 → Inkdown。

> 安装包未做代码签名，若首次运行出现 SmartScreen 提示，点“更多信息 → 仍要运行”即可。

### macOS（Apple Silicon）

打开 `.dmg`，把 Inkdown 拖入 Applications。未做 Apple 公证，首次启动请**右键 → 打开**。

### Linux

```bash
sudo apt install ./Inkdown-*-linux-x86_64.deb
```

## 功能一览

- **编辑器**：Markdown 语法高亮、行号、软换行、查找、撤销/重做、格式化工具栏与快捷键
- **实时预览**：表格、任务列表、代码块、引用、本地图片、可缩放；**YAML frontmatter 渲染为元数据卡片**
- **预览内查找**：`Ctrl + F` 在预览里高亮并逐条跳转
- **粘贴图片**：截图后 `Ctrl + V` 自动存到 `assets/`（BMP 自动转 PNG），可自动上传图床
- **文档元数据**：`Ctrl + Shift + M` 可视化编辑 frontmatter 字段
- **本地文件夹**：文件树、新建/重命名/删除、拖拽 `.md` 或文件夹进窗口直接打开
- **图床**：CloudFlare-ImgBed、Lsky Pro
- **发布**：公众号草稿 API、博客园 MetaWeblog，知乎/头条/CSDN/简书/掘金富文本分发
- **AI 写作**：任意 OpenAI 兼容接口，拉取模型、可配置提示词，生成新文档或插入光标
- **备份与 Git**：zip 备份、保存后自动同步
- **安全退出**：未保存文档关闭前确认；设置改动即时自动保存

## 系统要求

- Windows 10/11 x64；macOS 12+（Apple Silicon）；Ubuntu 22.04+（其他发行版可尝试）
- 首次运行无需额外运行时

## 配置

配置保存在：

- Windows：`%APPDATA%\Inkdown\config.json`
- macOS：`~/Library/Application Support/Inkdown/config.json`
- Linux：`~/.config/Inkdown/config.json`

AppID / Token 等密钥以明文保存在本机，请勿提交到公开仓库。

---

## 变更列表
