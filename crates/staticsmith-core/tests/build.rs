//! 端到端构建测试：脚手架 → 全量生成 → 级联增量更新。

use std::path::Path;

use staticsmith_core::{build::BuildMode, scaffold, Builder};

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

    // 模拟在可视化编辑器里改动导航菜单。
    let header = dir.path().join("templates/components/header.html");
    let source = std::fs::read_to_string(&header).unwrap();
    std::fs::write(&header, source.replace("首页", "回到首页")).unwrap();

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

    let html = builder.preview("posts/hello-staticsmith.md").unwrap();
    assert!(html.contains("site-header"), "预览应包含父级布局");
    assert!(!dir.path().join("dist").exists(), "预览不应写盘");
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
