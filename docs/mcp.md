# MCP：让 AI Agent 操作站点

StaticSmith 内置 MCP（Model Context Protocol）服务端，Agent 可以用工具调用的方式
读内容、改模板、查看级联影响、生成产物、执行发布。

```bash
# stdio（默认）：客户端把本进程当子进程拉起
staticsmith mcp --project ./my-site

# HTTP：POST /mcp 与 GET /sse 两套端点
staticsmith mcp --sse --port 5330 --project ./my-site

# 放开写入 / 发布（默认都关）
staticsmith mcp --allow-write --allow-deploy --project ./my-site
```

## 权限分三级，默认只读

Agent 误删内容或误发布线上站点的代价，远高于少几个工具，因此：

- 默认：只读工具
- `--allow-write`：写内容与模板、生成产物
- `--allow-deploy`：执行发布

`tools/list` 只返回当前允许的工具——让 Agent 看到用不了的工具只会导致反复试错。
被拒绝的调用返回 `isError: true` 的结果并说明需要哪个开关，而不是静默失败。

## 工具

只读：

- `site_info`：配置、目录、统计、最近构建记录。建议第一步就调它
- `list_pages`：内容页列表，可按栏目过滤
- `read_content`：读原文（含 `+++` front matter）
- `search_content`：标题与正文的子串搜索，返回命中位置（`field`）与片段。
  搜的是**读者看到的文字**（正文先从 HTML 还原成纯文本再搜，所以搜 `img`
  不会命中每一张图片），判断与桌面端侧栏共用一份实现
- `list_templates`：模板及其角色与直接依赖
- `read_template`：读模板源码
- `build_plan`：待生成范围与级联影响，**改模板前先看它**
- `audit_seo`：SEO 体检清单，可按 `severity`（error/warn/hint）或 `source` 过滤。
  与桌面端「体检」标签页同源，见 [SEO 与内容运营](seo.md)
- `audit_media`：媒体资源体检（没人引用的文件 / 引用了却不存在的地址）。
  删文件不开给 Agent，只能人在界面里确认
- `audit_links`：站内死链体检。读产物，所以先 `build_site`；站外链接只计数，不发网络请求
- `list_sections`：栏目清单（标题、简介、排序权重、地址、直属篇数、草稿数、
  有没有索引页、子栏目）。没有索引页的栏目打不开列表页，报告里会点出来

需要 `--allow-write`：

- `create_content`：按标题生成 front matter 骨架，默认草稿
- `write_content`：整文件覆盖写入，返回增量计划
- `patch_front_matter`：只改指定字段（标题、描述、关键词、标签、日期、旧地址、草稿…），
  正文与其他键原样保留。**补 SEO 字段用这个**，比整文覆盖安全
- `create_section`：建一层栏目目录并写好索引页（没有索引页的栏目打不开列表页）。
  栏目的标题、简介、排序权重就是索引页的 front matter，用 `patch_front_matter`
  改即可，不另开一个工具
- `rename_section`：栏目改名。默认 `keep_aliases = true`，给每篇文章补旧地址，
  构建后老链接经重定向页继续可用
- `move_content`：把一篇内容搬到另一个栏目，默认补旧地址。栏目索引页不能搬；
  批量搬就逐篇调用
- `replace_text`：跨文件替换**正文**里的一段文字（改称呼、统一术语）。
  front matter 一个字节都不碰——改字段用 `patch_front_matter`；纯文本，不支持正则。
  **默认 `dry_run = true`**：先回传哪几篇、共几处、每处前后对照，
  确认后再带 `dry_run: false` 落盘。正文替换没有撤销，默认值取那个「说错了也没损失」的方向
- `delete_content`：删除内容（标记为 destructive）
- `write_template`：写模板，返回级联影响范围
- `build_site`：生成产物


需要 `--allow-deploy`：

- `deploy_site`：按配置发布（标记为 destructive），凭证取自环境变量

`delete_content` 与 `deploy_site` 带 `destructiveHint`，支持该标注的客户端会要求人工确认。

## 传输与端点

- `POST /mcp`：Streamable HTTP 的最简形态，请求体是 JSON-RPC，响应直接返回 JSON。
  只含通知时返回 202 空响应
- `GET /sse`：旧版 HTTP+SSE（协议版本 2024-11-05）的服务端流，
  首个事件是 `endpoint`，告诉客户端往 `/messages?sessionId=…` POST
- `POST /messages?sessionId=…`：旧版传输的客户端上行，本次 HTTP 只回 202，
  真正的 JSON-RPC 响应经 SSE 流推回
- `GET /`：人可读的端点说明与当前权限

旧的 HTTP+SSE 传输已被规范标记为 deprecated，但现存客户端里仍有大量只实现了它，
所以两套并存——规范本身也是这么建议做向后兼容的。

### 只认本机进程，不认网页

这个端口**没有鉴权**，只绑回环地址，唯一的边界是「谁能发出这个请求」。所以有两道门：

- **带 `Origin` 请求头的一律 403。** MCP 客户端是本机进程，不会带这个头；
  网页里的 `fetch` / `EventSource` 一定会带。所以「带了 Origin」等价于「这是网页发来的」。
  不放行「本机来源的 Origin」——恶意页面完全可以由本机另一个服务托管。
- **请求体必须是 `application/json`，否则 415。** `text/plain` 属于 CORS 简单请求，
  **不触发预检**，网页可以朝 `127.0.0.1` 的每个端口盲打而浏览器不拦（读不到响应，
  但写操作已经发生了）。要求 JSON 之后浏览器必须先发 `OPTIONS`，而这里从不放行预检。

两道门是独立的，缺任一条都能被绕过。自己写客户端时记得别带 `Origin`、记得设 `Content-Type`。


协议版本协商：客户端在 `initialize` 里声明的版本若在支持列表内就原样回应，
否则回落到本服务端实现的**最新**版本 `2025-11-25`。
当前支持 `2025-11-25` / `2025-06-18` / `2025-03-26` / `2024-11-05`。

`SUPPORTED_PROTOCOL_VERSIONS` 必须严格按新到旧排列，回落取第一项。新增版本要插到
对应位置，**不能图省事追加在末尾**——`2025-11-25` 曾经被追加在末位，回落值就悄悄退回了
`2025-06-18`。现在有一条测试守着这个顺序。

规范生态里的当前版本是 `2026-07-28`（无状态内核、去掉 `initialize` 握手、
改用 `server/discover`），本服务端**尚未实现**：那是一次破坏性迁移，
不是往数组里加个字符串的事。旧版本按规范的弃用政策至少还能用 12 个月。

## 客户端配置

**从桌面端开**（不用终端）：「设置 → AI 接入」勾好权限点「开启 AI 接入」，
面板上会给出端点地址与一个「复制端点地址」按钮，把它填进客户端的 HTTP 配置即可。
端点只监听 127.0.0.1，跟着当前站点活——换站点或关掉应用就停。
权限是启动参数：改权限会重启这个端点，界面上显示的权限从正在跑的服务端读回来，
不会出现「勾了写入、其实只读」。

注意 Agent 改的是磁盘上的文件，桌面端不会自动跟着变：改完之后界面顶部会出现
「检测到磁盘上被外部修改」，点一下刷新。这不是疏忽——两边共用一个构建器就得把
每次 Agent 调用塞进界面的锁里，一次 `build_site` 会让界面卡住整段时间。

Claude Desktop / Cursor 这类拉起子进程的客户端（stdio）：

```json
{
  "mcpServers": {
    "staticsmith": {
      "command": "staticsmith",
      "args": ["mcp", "--project", "/absolute/path/to/my-site", "--allow-write"]
    }
  }
}
```

支持远程 MCP 的客户端填 SSE 地址：`http://127.0.0.1:5330/sse`；
支持 Streamable HTTP 的填 `http://127.0.0.1:5330/mcp`。

## 安全边界

- **只绑 127.0.0.1**。这个端点能读写站点源文件，绝不该暴露到局域网。
  需要远程访问请自己套一层带认证的反向代理
- **模板名与栏目名都会被清洗**。Agent 传 `../../evil.html` 只会写到
  `templates/evil.html`，写不出模板目录
- **凭证不经过 Agent**。发布凭证只从环境变量读（见 [命令行工具](cli.md#凭证只读环境变量)），
  Agent 既拿不到也传不进来
- **stdout 只跑协议**。走 stdio 时日志一律走 stderr，否则会污染 JSON-RPC 流

## 典型对话流程

一个「给全站头部加一个导航项」的请求，Agent 通常这样走：

1. `site_info` 了解站点与目录结构
2. `list_templates` 找到 `components/header.html`
3. `read_template` 读现有源码
4. `write_template` 写回改动 → 返回里带 `affected_templates` 与受影响页面数
5. `build_site` 生成 → 返回渲染页数与耗时

第 4 步的返回值让 Agent 能直接向用户交代「这次改动影响了多少页面」，
而不用它自己去推断依赖关系。
