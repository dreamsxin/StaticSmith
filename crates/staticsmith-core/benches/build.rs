//! 构建管线的基准测试。
//!
//! 为什么要有：产品文档里一直写着「5000 篇约 2.1 秒」这类**设计目标**，而
//! `docs/architecture.md` 自己承认没有实测。没有基准的性能判断只能靠读代码猜，
//! 而猜错的代价是把时间花在不痛的地方。
//!
//! 这里量三件事：
//!
//! - 全量构建在 100 / 400 篇下的耗时。**两个规模是有意的**：只有一个点没法回答
//!   「随篇数是线性还是更差」，而那正是决定要不要动渲染上下文的关键
//!   （每渲染一页都把整站 `pages` 重新序列化一遍，理论上是 O(n²)）。
//! - 增量构建改一篇的耗时：日常写作的真实回路。
//! - 空跑一次增量（什么都没改）的耗时：`serve` 监听下每次保存都会走这条路。
//!
//! 跑法：`cargo bench -p staticsmith-core`。结果记在 `docs/architecture.md`。

use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use staticsmith_core::build::BuildMode;
use staticsmith_core::{scaffold, Builder};

/// 铺一个有 `pages` 篇文章的站点。
///
/// 正文长度按真实文章给（约 2 KB）：正文进 `Page::content`，而渲染上下文里
/// 带着整站 `pages`，正文长度直接决定那份数据有多大——用一行字测不出问题。
fn project(pages: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("临时目录");
    scaffold::init_project(dir.path(), Some("基准站点"), scaffold::Preset::Docs).expect("铺站点");

    let body = "这是一段用来撑出真实体积的正文。".repeat(60);
    for i in 0..pages {
        let source = format!(
            "+++\ntitle = \"第 {i} 篇\"\ndate = \"2026-01-{:02}\"\ntags = [\"标签{}\"]\n+++\n\n{body}\n",
            (i % 28) + 1,
            i % 7
        );
        std::fs::write(dir.path().join(format!("content/posts/p{i}.md")), source).expect("写文章");
    }
    dir
}

fn full_build(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("全量构建");
    // 文件 IO 与 SQLite 落盘让单次迭代在几百毫秒量级，默认 100 次采样要跑很久。
    group.sample_size(10);

    for pages in [100usize, 400] {
        group.bench_with_input(BenchmarkId::from_parameter(pages), &pages, |b, &pages| {
            b.iter_batched(
                || project(pages),
                |dir| {
                    let mut builder = Builder::open(dir.path()).expect("打开项目");
                    builder.build(BuildMode::Full).expect("构建");
                    dir
                },
                criterion::BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn incremental_build(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("增量构建");
    group.sample_size(10);

    // 改一篇：日常写作的回路。站点规模固定 400 篇，看的是「跟站点大小有多少关系」。
    group.bench_function("改一篇（400 篇站点）", |b| {
        b.iter_batched(
            || {
                let dir = project(400);
                {
                    let mut builder = Builder::open(dir.path()).expect("打开项目");
                    builder.build(BuildMode::Full).expect("首次构建");
                }
                dir
            },
            |dir| {
                let article = dir.path().join("content/posts/p7.md");
                let source = std::fs::read_to_string(&article).expect("读文章");
                std::fs::write(&article, format!("{source}\n补一段。\n")).expect("改文章");
                let mut builder = Builder::open(dir.path()).expect("打开项目");
                builder.build(BuildMode::Incremental).expect("增量构建");
                dir
            },
            criterion::BatchSize::PerIteration,
        );
    });

    // 什么都没改：`serve` 监听下每次保存都会走一遍，代价应当接近于零。
    group.bench_function("空跑（400 篇站点）", |b| {
        b.iter_batched(
            || {
                let dir = project(400);
                {
                    let mut builder = Builder::open(dir.path()).expect("打开项目");
                    builder.build(BuildMode::Full).expect("首次构建");
                }
                dir
            },
            |dir| {
                let mut builder = Builder::open(dir.path()).expect("打开项目");
                builder.build(BuildMode::Incremental).expect("增量构建");
                dir
            },
            criterion::BatchSize::PerIteration,
        );
    });

    group.finish();
}

/// 单独量一次「打开项目」：解析全站 front matter 与模板，增量构建的固定开销。
fn open_project(criterion: &mut Criterion) {
    let dir = project(400);
    let root: &Path = dir.path();
    let mut group = criterion.benchmark_group("打开项目");
    group.sample_size(10);
    group.bench_function("400 篇", |b| {
        b.iter(|| Builder::open(root).expect("打开项目"));
    });
    group.finish();
}

criterion_group!(benches, full_build, incremental_build, open_project);
criterion_main!(benches);
