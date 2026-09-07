# 统一模板与级联更新

## 目录约定

模板名 = 相对 `templates/` 的正斜杠路径，首层目录决定它的角色：

- `layouts/`：主布局。定义 HTML 骨架与 `{% block %}` 占位
- `components/`：全局共享组件。头部、底部、侧边栏、分页条
- `pages/`：页面模板。继承布局并填充主体
- 其他目录：片段与宏库，按 `partial` 处理

```
templates/
├── layouts/base.html
├── components/{header,footer,sidebar,pagination}.html
└── pages/{index,list,post}.html
```

## 页面模板的选择

front matter 里的 `template` 优先。缺省时按目录约定推导（`content.rs::default_template`）：

- 根目录的 `index.md` / `_index.md` → `pages/index.html`
- 子目录的 `index.md` / `_index.md` → `pages/list.html`
- 其余 → `pages/post.html`

## 依赖提取

`templates.rs::extract_dependencies` 用正则扫描 `extends` / `include` / `import` 的**字面量**
模板名，支持 `{% include ["a.html", 'b.html'] %}` 这种候选列表写法。

动态表达式（`{% include page.template %}`）无法静态分析，会被忽略——这类模板的变更
不会被级联捕捉到，需要手动「生成全站」。

## 级联判定过程

1. 加载模板时计算每个文件的内容哈希，与索引中上次的哈希比较，得到 `changed_templates`
2. 在依赖图上沿反向边求传递闭包，得到 `affected_templates`
3. 索引里筛出 `template` 落在闭包内的页面，加入待生成列表

例：`components/header.html` 被 `layouts/base.html` 引用，而 `pages/index.html`、
`pages/post.html` 都继承 `base`。改动 header 后，闭包包含 base 与两个页面模板，
于是全站页面都进入待生成列表。而改动 `components/pagination.html` 时，
闭包只覆盖引用了它的列表页模板，文章页不受影响。

依赖图带环时不会死循环，`TemplateGraph::tree` 会把成环节点标记为 `cyclic`，界面据此提示。

## 渲染上下文

所有模板都能拿到：

- `site`：`[site]` 配置段（`title` / `description` / `base_url` / `language` / `extra`）
- `build`：`[build]` 配置段
- `taxonomy`：`[taxonomy]` 配置段，用来判断是否渲染标签入口
- `page`：当前页面（`title` / `date` / `tags` / `url` / `content` / `section` / `extra` …）
- `pages`：全站可发布页面数组
- `generator`：`"StaticSmith 2.0"`

文章页额外获得：

- `tag_links`：`[{ name, url }]`，当前页面的标签及其标签页地址。
  标签功能关闭时为空数组，模板可回落为纯文本

栏目索引页（`is_index` 为真）额外获得：

- `items`：当前分页的条目（同栏目下的非索引页，按 `weight` 升序、日期降序）
- `pagination`：分页上下文

标签总览页（`pages/tags.html`）：

- `terms`：`[{ name, slug, url, count }]`，按篇数降序、同数按名称升序
- `page`：合成对象（`title` 取 `taxonomy.title`、`url` 为 `/tags/`），便于复用主布局

单标签页（`pages/tag.html`）：

- `term`：当前标签
- `terms`：全部标签（可用于侧边栏标签云）
- `items` 与 `pagination`：与栏目列表页同构，分页大小取 `build.page_size`

## 分页

分页由后端计算并注入，模板只负责渲染。`Pagination` 提供：

- `current_page` / `total_pages`
- `base_url`：栏目地址，如 `/posts/`
- `prev_url` / `next_url`：首尾页对应为 `null`
- `page_urls`：全部页码地址

输出位置：第 1 页写在栏目根（`posts/index.html`），第 N 页写到 `posts/page/N/index.html`。

因此修改 `components/pagination.html` 的文案或类名，所有列表页（首页、归档页、
自定义栏目页）会一起级联更新，交互保持一致。

## 内置过滤器

Tera 自带 `now()`、`date`、`slugify`、`truncate`、`safe` 等。额外注册（`filters.rs`）：

- `markdown`：把字符串按 Markdown 渲染，用于 front matter 里的富文本片段
- `absolute_url(base=...)`：拼接站点绝对地址，自动处理斜杠

注意：Tera 默认对 `.html` 模板自动转义，输出引擎生成的 URL 时需要 `| safe`，
否则 `/` 会被转成 `&#x2F;`。内置模板已经这样处理。

## 手动改模板

后端有文件监听服务。外部编辑器修改 `templates/` 下的文件后，界面会收到
`project://changed` 事件并提示刷新组件树；刷新后重新计算影响范围。
监听不会自动覆盖你正在编辑的内容。
