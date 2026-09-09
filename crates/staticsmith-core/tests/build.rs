//! 端到端构建测试：脚手架 → 全量生成 → 级联增量更新。

use std::path::Path;

use staticsmith_core::{build::BuildMode, scaffold, Builder, NewContent, OutputKind};

/// 新建一个临时项目并返回其根目录。
fn new_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    scaffold::init_project(dir.path(), Some("测试站点")).unwrap();
    dir
}

fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(root.join("dist").join(relative))
        .unwrap_or_else(|e| panic!("读取 dist/{relative} 失败: {e}"))
}

#[test]
fn full_build_renders_all_pages_through_the_layout() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();

    let report = builder.build(BuildMode::Full).unwrap();
    assert_eq!(report.pages_rendered, 4, "首页 + 关于 + 归档 + 一篇文章");
    assert!(report.assets_copied >= 1, "主题 CSS 应被复制");
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );

    // 布局继承生效：页面里同时出现头部与底部组件的内容。
    let index = read(dir.path(), "index.html");
    assert!(index.contains("测试站点"));
    assert!(index.contains("site-header"));
    assert!(index.contains("site-footer"));

    // pretty URL 输出结构。
    assert!(dir.path().join("dist/posts/index.html").exists());
    assert!(dir
        .path()
        .join("dist/posts/hello-staticsmith/index.html")
        .exists());
    assert!(dir.path().join("dist/about/index.html").exists());
    assert!(dir.path().join("dist/css/main.css").exists());

    // 文章正文由 Markdown 渲染。
    let post = read(dir.path(), "posts/hello-staticsmith/index.html");
    assert!(post.contains("<h2>依赖图</h2>"));
}

#[test]
fn navigation_comes_from_the_config_menu() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    // 脚手架自带四项菜单；这里加一条站外链接，验证 blank 与排序都落到产物里
    std::fs::write(
        &config_path,
        format!("{config}\n[[menu]]\nname = \"源码\"\nurl = \"https://example.com/repo\"\nweight = 0\nblank = true\n"),
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let index = read(dir.path(), "index.html");
    let nav_start = index.find("site-header__nav").expect("导航容器");
    let nav = &index[nav_start..];
    let source_at = nav.find("源码").expect("站外链接进了导航");
    let home_at = nav.find("首页").expect("首页仍在导航里");
    assert!(source_at < home_at, "weight = 0 应排在 weight = 1 之前");
    assert!(nav.contains(r#"target="_blank" rel="noopener""#));

    // 没有菜单的站点（升级上来的老站）退回模板内置链接，导航不会突然空掉
    let menu_at = config.find("[[menu]]").expect("脚手架自带菜单");
    let deploy_at = config.find("[deploy]").expect("脚手架自带发布段");
    let without_menu = format!("{}{}", &config[..menu_at], &config[deploy_at..]);
    std::fs::write(&config_path, without_menu).unwrap();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();
    let index = read(dir.path(), "index.html");
    assert!(index.contains(r#"href="/posts/""#), "内置链接应回来");
    assert!(!index.contains("源码"), "配置里的菜单已删除");
}

#[test]
fn second_incremental_build_is_a_no_op() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let plan = builder.plan(BuildMode::Incremental).unwrap();
    assert!(
        plan.is_empty(),
        "无改动时不应有待生成页面: {:?}",
        plan.pages
    );
    assert!(plan.changed_templates.is_empty());
}

#[test]
fn editing_a_global_component_cascades_to_every_page() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    // 模拟在可视化编辑器里改动导航组件（菜单文案在配置里，这里改的是模板本身）。
    let header = dir.path().join("templates/components/header.html");
    let source = std::fs::read_to_string(&header).unwrap();
    std::fs::write(
        &header,
        source.replace("</nav>", r#"<a href="/">回到首页</a></nav>"#),
    )
    .unwrap();

    builder.reload().unwrap();
    let plan = builder.plan(BuildMode::Incremental).unwrap();
    assert_eq!(
        plan.changed_templates,
        vec!["components/header.html".to_string()]
    );
    assert!(plan
        .affected_templates
        .contains(&"layouts/base.html".to_string()));
    assert_eq!(
        plan.pages.len(),
        plan.total_pages,
        "所有页面都继承 base，应全部受影响"
    );

    let report = builder.build(BuildMode::Incremental).unwrap();
    assert_eq!(report.pages_rendered, plan.total_pages);
    assert!(read(dir.path(), "posts/hello-staticsmith/index.html").contains("回到首页"));
}

#[test]
fn editing_one_article_rebuilds_only_that_page() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let article = dir.path().join("content/posts/hello-staticsmith.md");
    let source = std::fs::read_to_string(&article).unwrap();
    std::fs::write(&article, format!("{source}\n\n补充一段说明。\n")).unwrap();

    builder.reload().unwrap();
    let plan = builder.plan(BuildMode::Incremental).unwrap();
    assert_eq!(plan.pages, vec!["posts/hello-staticsmith.md".to_string()]);

    let report = builder.build(BuildMode::Incremental).unwrap();
    assert_eq!(report.pages_rendered, 1);
    assert!(read(dir.path(), "posts/hello-staticsmith/index.html").contains("补充一段说明"));
}

#[test]
fn deleting_content_removes_its_output() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();
    assert!(dir.path().join("dist/about/index.html").exists());

    std::fs::remove_file(dir.path().join("content/about.md")).unwrap();
    builder.reload().unwrap();

    let plan = builder.plan(BuildMode::Incremental).unwrap();
    assert_eq!(plan.orphaned_pages, vec!["about.md".to_string()]);

    let report = builder.build(BuildMode::Incremental).unwrap();
    assert_eq!(report.removed_files, vec!["about/index.html".to_string()]);
    assert!(!dir.path().join("dist/about/index.html").exists());
}

#[test]
fn list_pages_are_paginated_with_the_shared_component() {
    let dir = new_project();

    // 把分页大小改成 2，并补足文章数量以产生第 2 页。
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("page_size = 10", "page_size = 2"),
    )
    .unwrap();
    for i in 1..=3 {
        std::fs::write(
            dir.path().join(format!("content/posts/p{i}.md")),
            format!("+++\ntitle = \"文章 {i}\"\ndate = \"2026-09-0{i}\"\n+++\n正文 {i}\n"),
        )
        .unwrap();
    }

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    // 4 篇文章 / 每页 2 篇 = 2 页。
    let first = read(dir.path(), "posts/index.html");
    assert!(first.contains("class=\"pagination\""));
    assert!(first.contains("/posts/page/2/"));

    let second = read(dir.path(), "posts/page/2/index.html");
    assert!(second.contains("pagination__prev"));
    assert!(dir.path().join("dist/posts/page/2/index.html").exists());
    assert!(!dir.path().join("dist/posts/page/3").exists());
}

#[test]
fn preview_renders_without_writing_files() {
    let dir = new_project();
    let builder = Builder::open(dir.path()).unwrap();

    let (html, inlined) = builder.preview("posts/hello-staticsmith.md").unwrap();
    assert!(html.contains("site-header"), "预览应包含父级布局");
    assert!(!dir.path().join("dist").exists(), "预览不应写盘");
    // 骨架站点的样式表是本地文件，应当已经内联成 <style>——srcdoc 沙箱取不到磁盘文件
    assert!(inlined.replaced > 0, "本地样式应被内联：{inlined:?}");
    assert!(!html.contains("<link rel=\"stylesheet\""), "{html}");
}

#[test]
fn draft_pages_are_not_published() {
    let dir = new_project();
    std::fs::write(
        dir.path().join("content/posts/wip.md"),
        "+++\ntitle = \"草稿\"\ndraft = true\n+++\n未完成\n",
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();
    assert!(!dir.path().join("dist/posts/wip/index.html").exists());
}

#[test]
fn future_dated_posts_wait_until_their_date_when_scheduling_is_on() {
    let dir = new_project();
    // 脚手架里写着 publish_future = true（写什么日期都发），这里改成定时发布
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("publish_future = true", "publish_future = false"),
    )
    .unwrap();

    std::fs::write(
        dir.path().join("content/posts/scheduled.md"),
        "+++\ntitle = \"排到下周\"\ndate = \"2099-01-01\"\n+++\n\n还没到点。\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("content/posts/released.md"),
        "+++\ntitle = \"已经发了\"\ndate = \"2020-01-01\"\n+++\n\n早就发了。\n",
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    assert!(
        !dir.path().join("dist/posts/scheduled/index.html").exists(),
        "未到日期的文章不该进产物"
    );
    assert!(dir.path().join("dist/posts/released/index.html").exists());

    // 列表页也不该出现它，否则点进去是 404
    let list = read(dir.path(), "posts/index.html");
    assert!(!list.contains("排到下周"), "{list}");

    // 界面与自动化用同一个判断
    let published: Vec<&str> = builder
        .published_pages()
        .iter()
        .map(|p| p.source.as_str())
        .collect();
    assert!(!published.contains(&"posts/scheduled.md"), "{published:?}");
    assert!(published.contains(&"posts/released.md"));

    // 关掉定时发布（回到默认）后立刻发布
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("publish_future = false", "publish_future = true"),
    )
    .unwrap();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();
    assert!(dir.path().join("dist/posts/scheduled/index.html").exists());
}

#[test]
fn aliases_emit_redirect_pages_so_old_links_keep_working() {
    let dir = new_project();
    std::fs::write(
        dir.path().join("content/posts/renamed.md"),
        "+++\ntitle = \"换过地址的文章\"\nslug = \"new-slug\"\naliases = [\"/posts/old-slug/\", \"legacy.html\"]\n+++\n\n正文。\n",
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    // 新地址正常生成
    assert!(dir.path().join("dist/posts/new-slug/index.html").is_file());

    // 旧地址变成重定向页：立刻跳走、canonical 指向新地址、自己不被收录
    let stub = read(dir.path(), "posts/old-slug/index.html");
    assert!(
        stub.contains(r#"content="0; url=/posts/new-slug/""#),
        "{stub}"
    );
    assert!(stub.contains(r#"<link rel="canonical" href="/posts/new-slug/""#));
    assert!(stub.contains(r#"content="noindex""#));
    assert!(
        stub.contains(r#"<a href="/posts/new-slug/""#),
        "要留一条手点的链接"
    );

    // 带扩展名的旧地址原样落成文件
    assert!(dir.path().join("dist/legacy.html").is_file());

    // 死链体检因此看不到问题：旧地址在产物里是真实存在的
    let links = builder.audit_links().unwrap();
    assert!(links.built);
    assert!(links.broken.is_empty(), "{:?}", links.broken);
}

#[test]
fn build_history_is_persisted_in_the_index() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();
    builder.build(BuildMode::Incremental).unwrap();

    let builds = builder.index().recent_builds(10).unwrap();
    assert_eq!(builds.len(), 2);
    assert_eq!(builds[0].mode, "incremental");
    assert_eq!(builds[1].mode, "full");
    assert!(dir.path().join(".staticsmith/index.db").exists());
}

/// 一张最小的合法 PNG 头部，足以让扩展名嗅探与哈希命名生效。
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00pasted image bytes";

#[test]
fn pasted_image_lands_in_static_dir_and_gets_copied_to_dist() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();

    let saved = builder.save_asset(PNG, "屏幕截图.png").unwrap();

    // 落盘位置由 static_dir + assets.dir + 哈希分片决定。
    assert!(saved.relative_path.starts_with("images/"));
    assert_eq!(saved.url, format!("/{}", saved.relative_path));
    let on_disk = saved
        .relative_path
        .split('/')
        .fold(dir.path().join("static"), |acc, s| acc.join(s));
    assert!(on_disk.is_file());

    // 在文章里引用它，构建后资源与页面同时出现在产物中。
    let article = dir.path().join("content/posts/with-image.md");
    std::fs::write(
        &article,
        format!("+++\ntitle = \"带图\"\n+++\n\n![截图]({})\n", saved.url),
    )
    .unwrap();
    builder.reload().unwrap();
    builder.build(BuildMode::Full).unwrap();

    let html = read(dir.path(), "posts/with-image/index.html");
    assert!(html.contains(&saved.url), "页面应引用资源地址");
    let copied = saved
        .relative_path
        .split('/')
        .fold(dir.path().join("dist"), |acc, s| acc.join(s));
    assert!(copied.is_file(), "资源应被复制到 {}", copied.display());
}

#[test]
fn pasting_the_same_image_twice_stores_one_copy() {
    let dir = new_project();
    let builder = Builder::open(dir.path()).unwrap();

    let first = builder.save_asset(PNG, "a.png").unwrap();
    let second = builder.save_asset(PNG, "b.png").unwrap();

    assert_eq!(first.url, second.url);
    assert!(second.deduplicated);
    assert_eq!(builder.assets().unwrap().len(), 1, "索引里只应有一条记录");
}

#[test]
fn asset_directory_is_configurable() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config
            .replace("static_dir = \"./static\"", "static_dir = \"./public\"")
            .replace("dir = \"images\"", "dir = \"media/2026\"")
            .replace("shard = true", "shard = false"),
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    let saved = builder.save_asset(PNG, "cover.png").unwrap();

    assert_eq!(
        saved.relative_path,
        format!("media/2026/{}", saved.file_name)
    );
    assert_eq!(saved.url, format!("/media/2026/{}", saved.file_name));
    assert!(dir
        .path()
        .join("public/media/2026")
        .join(&saved.file_name)
        .is_file());

    builder.build(BuildMode::Full).unwrap();
    assert!(dir
        .path()
        .join("dist/media/2026")
        .join(&saved.file_name)
        .is_file());
}

#[test]
fn full_build_emits_sitemap_and_atom_feed() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    let report = builder.build(BuildMode::Full).unwrap();

    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let sitemap = read(dir.path(), "sitemap.xml");
    assert!(sitemap.contains("<loc>https://example.com/</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/posts/hello-staticsmith/</loc>"));

    let feed = read(dir.path(), "feed.xml");
    assert!(feed.contains("<feed xmlns=\"http://www.w3.org/2005/Atom\""));
    assert!(feed.contains("rel=\"self\" href=\"https://example.com/feed.xml\""));
    assert!(feed.contains("<title>统一模板与级联更新是怎么工作的</title>"));
    // 栏目索引页不进订阅。
    assert!(!feed.contains("<title>文章归档</title>"));
}

#[test]
fn site_files_can_be_disabled_and_warn_without_base_url() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("base_url = \"https://example.com\"", "base_url = \"\""),
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    let report = builder.build(BuildMode::Full).unwrap();

    assert!(!dir.path().join("dist/sitemap.xml").exists());
    assert!(!dir.path().join("dist/feed.xml").exists());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("base_url"));
}

#[test]
fn full_build_emits_tag_pages_and_links_them_from_posts() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    // 总览页列出标签与篇数
    let overview = read(dir.path(), "tags/index.html");
    assert!(overview.contains("模板"));
    assert!(overview.contains("增量构建"));
    assert!(overview.contains("/tags/模板/"));

    // 单标签页列出该标签下的文章
    let term = read(dir.path(), "tags/模板/index.html");
    assert!(term.contains("统一模板与级联更新是怎么工作的"));
    assert!(term.contains("共 1 篇"));

    // 文章页的标签变成可点链接
    let post = read(dir.path(), "posts/hello-staticsmith/index.html");
    assert!(
        post.contains("href=\"/tags/模板/\""),
        "文章页应链接到标签页"
    );
}

#[test]
fn built_pages_carry_seo_meta_and_audit_sees_the_same_data() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let post = read(dir.path(), "posts/hello-staticsmith/index.html");
    assert!(
        post.contains(
            r#"<link rel="canonical" href="https://example.com/posts/hello-staticsmith/""#
        ),
        "缺少 canonical，或地址被 Tera 转义：{post}"
    );
    assert!(post.contains(r#"<meta name="keywords""#), "缺少 keywords");
    assert!(
        post.contains(r#"property="og:title""#),
        "缺少 Open Graph 标题"
    );
    assert!(
        post.contains(r#"rel="alternate" type="application/atom+xml""#),
        "缺少订阅源声明"
    );

    // 体检读的是同一批已解析页面，不依赖产物，因此保存后立刻可用
    let report = builder.audit_seo();
    assert!(report.checked > 0);
    assert!(
        report
            .issues
            .iter()
            .all(|i| i.code != "site.base_url_missing"),
        "脚手架配了 base_url，不该报缺失"
    );
}

#[test]
fn outputs_list_exposes_generated_pages_that_have_no_source_file() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();

    // 还没生成时不应报错，只是空清单——界面据此提示「先生成一次」。
    assert!(builder.outputs().unwrap().is_empty());

    builder.build(BuildMode::Full).unwrap();
    let outputs = builder.outputs().unwrap();

    let url_of = |kind: OutputKind| -> Vec<&str> {
        outputs
            .iter()
            .filter(|o| o.kind == kind)
            .map(|o| o.url.as_str())
            .collect()
    };

    // 标签页只存在于产物里，内容树看不到，这是它进入界面的唯一入口。
    let taxonomy = url_of(OutputKind::Taxonomy);
    assert!(taxonomy.contains(&"/tags/"), "缺少标签总览：{taxonomy:?}");
    assert!(
        taxonomy.contains(&"/tags/模板/"),
        "缺少标签页：{taxonomy:?}"
    );

    assert!(url_of(OutputKind::Sitemap).contains(&"/sitemap.xml"));
    assert!(url_of(OutputKind::Feed).contains(&"/feed.xml"));
    assert!(url_of(OutputKind::Page).contains(&"/"));
    // CSS 归到静态资源，不该混进页面列表
    assert!(url_of(OutputKind::Asset)
        .iter()
        .any(|u| u.ends_with(".css")));
}

#[test]
fn multiple_taxonomies_each_get_their_own_pages() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    // 用 [[taxonomies]] 覆盖单数写法：标签 + 分类两个维度
    std::fs::write(
        &config_path,
        format!(
            "{config}\n[[taxonomies]]\nname = \"tags\"\nslug = \"tags\"\ntitle = \"标签\"\n\
             list_template = \"pages/tags.html\"\nterm_template = \"pages/tag.html\"\n\n\
             [[taxonomies]]\nname = \"categories\"\nslug = \"categories\"\ntitle = \"分类\"\n\
             list_template = \"pages/tags.html\"\nterm_template = \"pages/tag.html\"\n"
        ),
    )
    .unwrap();

    std::fs::write(
        dir.path().join("content/posts/categorized.md"),
        "+++\ntitle = \"带分类的文章\"\ncategories = [\"工程实践\"]\ntags = [\"模板\"]\n+++\n\n正文。\n",
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    // 两个维度各自成页
    assert!(read(dir.path(), "categories/index.html").contains("工程实践"));
    let term = read(dir.path(), "categories/工程实践/index.html");
    assert!(term.contains("带分类的文章"));
    assert!(read(dir.path(), "tags/index.html").contains("模板"));

    // 文章页把两个维度都渲染成链接
    let post = read(dir.path(), "posts/categorized/index.html");
    assert!(post.contains("href=\"/tags/模板/\""), "{post}");

    // 头部导航按配置列出全部维度
    assert!(
        post.contains("href=\"/categories/\""),
        "缺少分类入口：{post}"
    );
}

#[test]
fn taxonomy_can_be_disabled() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    // 配置里只有 [taxonomy] 段有 enabled 开关
    std::fs::write(
        &config_path,
        config.replace("enabled = true", "enabled = false"),
    )
    .unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    let report = builder.build(BuildMode::Full).unwrap();

    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!dir.path().join("dist/tags").exists());
    // 关闭后文章页回落为纯文本标签
    let post = read(dir.path(), "posts/hello-staticsmith/index.html");
    assert!(!post.contains("href=\"/tags/"));
}

#[test]
fn tag_pages_are_paginated_like_section_lists() {
    let dir = new_project();
    let config_path = dir.path().join("staticsmith.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("page_size = 10", "page_size = 1"),
    )
    .unwrap();

    // 再写两篇同标签文章，凑出 3 页
    for i in 1..=2 {
        std::fs::write(
            dir.path().join(format!("content/posts/t{i}.md")),
            format!(
                "+++\ntitle = \"标签文章 {i}\"\ndate = \"2026-09-0{i}\"\ntags = [\"模板\"]\n+++\n"
            ),
        )
        .unwrap();
    }

    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let first = read(dir.path(), "tags/模板/index.html");
    assert!(first.contains("共 3 篇"));
    assert!(first.contains("/tags/模板/page/2/"));
    assert!(dir
        .path()
        .join("dist/tags/模板/page/3/index.html")
        .is_file());
    assert!(!dir.path().join("dist/tags/模板/page/4").exists());
}

#[test]
fn missing_taxonomy_template_degrades_to_a_warning() {
    let dir = new_project();
    std::fs::remove_file(dir.path().join("templates/pages/tags.html")).unwrap();

    let mut builder = Builder::open(dir.path()).unwrap();
    let report = builder.build(BuildMode::Full).unwrap();

    // 缺模板不该让整次构建失败，页面照常生成
    assert!(dir.path().join("dist/index.html").is_file());
    assert!(!dir.path().join("dist/tags").exists());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("pages/tags.html"));
}

#[test]
fn create_content_writes_a_draft_skeleton_and_avoids_overwriting() {
    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();

    let source = builder
        .create_content(&NewContent::new("我的第一篇文章").in_section("posts"))
        .unwrap();
    assert_eq!(source, "posts/我的第一篇文章.md");

    // 新建后内容树里立刻能看到它，且默认是草稿。
    let page = builder
        .pages()
        .iter()
        .find(|p| p.source == source)
        .expect("新建的内容应出现在页面列表里");
    assert_eq!(page.title, "我的第一篇文章");
    assert!(page.draft);
    assert_eq!(page.template, "pages/post.html");

    // 同名再建一次不覆盖，追加序号。
    let second = builder
        .create_content(&NewContent::new("我的第一篇文章").in_section("posts"))
        .unwrap();
    assert_eq!(second, "posts/我的第一篇文章-2.md");

    // 草稿不进产物。
    builder.build(BuildMode::Full).unwrap();
    assert!(!dir
        .path()
        .join("dist/posts/我的第一篇文章/index.html")
        .exists());
}

#[test]
fn preview_server_serves_the_built_site() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let dir = new_project();
    let mut builder = Builder::open(dir.path()).unwrap();
    builder.build(BuildMode::Full).unwrap();

    let server = staticsmith_core::PreviewServer::start(&builder.paths.output, 0).unwrap();
    let mut stream = TcpStream::connect(server.addr()).unwrap();
    stream
        .write_all(b"GET /posts/hello-staticsmith/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("site-header"), "预览应返回完整页面");
}
