# SEO 与内容运营

站点做起来之后，日常工作从「搭站」变成「持续产出并优化」。这一部分回答两件事：
**每篇文章该填哪些字段**，以及**这些字段怎么批量补齐**——包括交给 AI Agent 去补。

## 字段

front matter 里与 SEO 直接相关的四个字段：

```toml
+++
title = "统一模板与级联更新是怎么工作的"
description = "讲清依赖图如何定位受影响页面，以及增量构建为什么只重算这些页面。"
keywords = ["静态站点", "增量构建"]
tags = ["模板", "增量构建"]
+++
```

- `title`：进 `<title>` 与 `og:title`。留空时回退到正文第一个 `#` 标题
- `description`：进 `<meta name="description">`、`og:description` 与订阅源。留空时回退到站点描述
- `keywords`：进 `<meta name="keywords">`。**留空时回退到 `tags`**
- `tags`：站内导航用，会生成 `/tags/<标签>/` 页面

关键词与标签分开的原因：标签是站内结构（多一个标签就多一个页面），关键词只是元信息。
多数文章两者一致，所以不写 `keywords` 就直接用 `tags`，不必填两遍。

界面里在编辑器标题栏点「属性」用表单填；改动只落到源文，仍需保存。

## 模板里的 SEO 输出

脚手架的 `layouts/base.html` 已经输出：描述、关键词、`canonical`、Open Graph、
Twitter Card、订阅源声明。`site.base_url` 为空时**跳过所有绝对地址**——
半截的 canonical 比没有更糟。

老项目的模板不会被自动改写（模板是你的代码）。想要这些标签，参考脚手架的
`crates/staticsmith-core/scaffold/templates/layouts/base.html` 补进自己的 `base.html`，
关键几行：

```html
<link rel="canonical" href="{{ page.url | absolute_url(base=site.base_url) | safe }}" />
<meta name="keywords" content="{{ page.keywords | join(sep=", ") }}" />
```

`| safe` 不能省：Tera 会把 URL 里的 `/` 转义成 `&#x2F;`。

## 体检

桌面端的「SEO」标签页、MCP 的 `audit_seo` 用的是同一份规则
（`crates/staticsmith-core/src/seo.rs`）。草稿不参与——它们不会发布。

规则与阈值：

- `title.missing`（必须修）：标题为空，正文里也没有一级标题
- `title.too_long`（建议修）：超过 60 字
- `description.missing`（建议修）/ `description.too_short`（可优化，< 40 字）/
  `description.too_long`（建议修，> 160 字）
- `keywords.missing`（可优化）：既没有关键词也没有标签
- `content.too_short`（可优化）：正文不足 200 字
- `title.duplicated` / `description.duplicated`（建议修）：与其他页面重复，报告里直接点出是哪几篇
- `site.base_url_missing`（建议修）/ `site.description_missing`（可优化）：站点级配置

阈值取自搜索结果与社交卡片的常见截断位置，中文按字计。分数（`score`）只用于
「今天比昨天好没好」的粗略对比，不对齐任何第三方评分。

## 让 AI Agent 批量补齐

程序本身不内置大模型调用：那会把 API Key、网络出口与内容隐私一起塞进桌面应用。
StaticSmith 的做法是把站点能力开放给你已经在用的 Agent（Claude Desktop、Cursor…），
由它生成文案，通过 MCP 写回来。

```bash
staticsmith mcp --project ./my-site --allow-write
```

一次典型的运营对话：

1. Agent 调 `audit_seo`，拿到按严重程度排序的清单
2. 对每篇有问题的内容调 `read_content` 读正文
3. 生成标题 / 描述 / 关键词，调 `patch_front_matter` 写回——
   只改指定字段，正文、注释与其他键原样保留
4. 再调一次 `audit_seo` 确认清单变短，必要时 `build_site`

可以直接把这段话丢给 Agent：

> 用 audit_seo 找出所有缺描述的文章，逐篇读正文后写一段 40-160 字的中文描述，
> 补 3-5 个关键词，用 patch_front_matter 写回。不要改正文。全部完成后再体检一次。

为什么补字段要用 `patch_front_matter` 而不是 `write_content`：后者要求 Agent
先读全文再原样吐回来，它一旦顺手「优化」了正文，改动范围就超出了你的预期。
`patch_front_matter` 从机制上限定了改动只发生在 front matter 的指定键上。

想先看不想让它改，就别给 `--allow-write`：`audit_seo` 是只读工具，
在默认权限下就能用。

## 其他运营相关产物

- `sitemap.xml`、`feed.xml`：`[build] generate_sitemap` / `generate_feed` 控制，
  都需要 `base_url`
- 标签页：`[taxonomy]` 控制，关掉后文章页也不会渲染标签链接（避免死链）
- 产物清单：「生成」标签页可以逐个打开检查，包括标签页与分页页
- 媒体资源体检：「SEO」标签页的「媒体资源」卡片列出没人引用的文件（可回收）
  与引用了却不存在的地址（页面上的破图）。删除需二次确认，且只能删资源目录内的文件；
  Agent 侧对应只读的 `audit_media`。细节见 [媒体资源与目录结构](assets.md)

