use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use tera::Tera;

use crate::error::{Error, Result};
use crate::graph::TemplateGraph;
use crate::util;

/// 模板文件扩展名白名单。
const TEMPLATE_EXTS: [&str; 3] = ["html", "tera", "xml"];

/// 模板名 → 绝对路径，并确保结果没有越出模板目录。
///
/// 与 [`crate::content::resolve_source`] 同一个道理：模板名可能来自 Agent 或 IPC 调用方。
/// 桌面端曾经直接 `templates.join(&name)`，而 MCP 那侧记得先清洗——**同一个操作两端
/// 一个洗一个不洗**，这正是把校验留给调用方自觉的下场。现在两端都走这里。
pub fn resolve_template(template_root: &Path, name: &str) -> Result<PathBuf> {
    let relative = util::sanitize_relative_dir(name);
    if relative.is_empty() {
        return Err(Error::Other(format!("模板名无效：{name}")));
    }
    let path = relative
        .split('/')
        .fold(template_root.to_path_buf(), |acc, part| acc.join(part));
    // 清洗已经剔掉 `..` 与空片段，这里再兜一层：Windows 盘符（`C:`）里不含斜杠，
    // 会被当成一个普通片段留下来，`join` 上去就把整条路径换掉了。
    if !path.starts_with(template_root) {
        return Err(Error::Other(format!("模板路径越出模板目录：{name}")));
    }
    Ok(path)
}

/// 模板在「统一模板」体系中的角色，由所在目录决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    /// `layouts/`：顶层主布局，定义 HTML 骨架与区块占位。
    Layout,
    /// `components/`：全局共享组件（header / footer / sidebar / pagination）。
    Component,
    /// `pages/`：页面模板，继承布局并填充主体。
    Page,
    /// 其他目录：片段、宏库等。
    Partial,
}

impl TemplateKind {
    fn from_name(name: &str) -> Self {
        match name.split('/').next() {
            Some("layouts") => Self::Layout,
            Some("components") => Self::Component,
            Some("pages") => Self::Page,
            _ => Self::Partial,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateInfo {
    /// Tera 模板名 = 相对模板目录的正斜杠路径，如 `components/header.html`。
    pub name: String,
    pub kind: TemplateKind,
    pub path: PathBuf,
    pub hash: String,
    /// 直接引用的模板（extends / include / import）。
    pub dependencies: Vec<String>,
}

/// 已加载的模板集合：Tera 实例 + 依赖图 + 元信息。
pub struct TemplateSet {
    pub tera: Tera,
    pub graph: TemplateGraph,
    infos: BTreeMap<String, TemplateInfo>,
}

impl TemplateSet {
    /// 扫描模板目录，编译 Tera 并构建依赖图。
    pub fn load(template_dir: &Path) -> Result<Self> {
        if !template_dir.exists() {
            return Err(Error::InvalidProject(format!(
                "模板目录不存在: {}",
                template_dir.display()
            )));
        }

        let mut raw = Vec::new();
        let mut infos = BTreeMap::new();
        let mut graph = TemplateGraph::new();

        for entry in walkdir::WalkDir::new(template_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let is_template = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| TEMPLATE_EXTS.contains(&e));
            if !is_template {
                continue;
            }
            let rel = path.strip_prefix(template_dir).unwrap_or(path);
            let name = util::to_slash(rel);
            let source = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
            let dependencies = extract_dependencies(&source);

            graph.set_dependencies(&name, dependencies.clone());
            infos.insert(
                name.clone(),
                TemplateInfo {
                    kind: TemplateKind::from_name(&name),
                    path: path.to_path_buf(),
                    hash: util::hash_str(&source),
                    dependencies,
                    name: name.clone(),
                },
            );
            raw.push((name, source));
        }

        let mut tera = Tera::default();
        tera.add_raw_templates(raw.iter().map(|(n, s)| (n.as_str(), s.as_str())))?;
        crate::filters::register(&mut tera);

        Ok(Self { tera, graph, infos })
    }

    pub fn get(&self, name: &str) -> Option<&TemplateInfo> {
        self.infos.get(name)
    }

    pub fn infos(&self) -> impl Iterator<Item = &TemplateInfo> {
        self.infos.values()
    }

    /// 全局共享组件列表，供前端「组件树 / 布局管理器」展示。
    pub fn components(&self) -> Vec<&TemplateInfo> {
        self.infos
            .values()
            .filter(|i| i.kind == TemplateKind::Component)
            .collect()
    }

    pub fn layouts(&self) -> Vec<&TemplateInfo> {
        self.infos
            .values()
            .filter(|i| i.kind == TemplateKind::Layout)
            .collect()
    }

    /// 当前磁盘状态与给定哈希表比较，返回内容发生变化的模板名。
    pub fn changed_since(&self, known: &BTreeMap<String, String>) -> Vec<String> {
        self.infos
            .values()
            .filter(|i| known.get(&i.name) != Some(&i.hash))
            .map(|i| i.name.clone())
            .collect()
    }

    pub fn hashes(&self) -> BTreeMap<String, String> {
        self.infos
            .values()
            .map(|i| (i.name.clone(), i.hash.clone()))
            .collect()
    }
}

/// 从模板源码中提取 `extends` / `include` / `import` 引用的模板名。
///
/// 只处理字面量字符串；动态表达式（如 `{% include page.tpl %}`）无法静态分析，
/// 这类模板需要用户显式声明依赖或退回全量构建。
pub fn extract_dependencies(source: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"\{%-?\s*(?:extends|include|import)\s+\[?\s*((?:"[^"]+"|'[^']+')(?:\s*,\s*(?:"[^"]+"|'[^']+'))*)"#,
        )
        .expect("模板依赖正则应当合法")
    });
    static LIT: OnceLock<Regex> = OnceLock::new();
    let lit = LIT.get_or_init(|| Regex::new(r#""([^"]+)"|'([^']+)'"#).expect("字面量正则应当合法"));

    let mut deps = Vec::new();
    for caps in re.captures_iter(source) {
        for l in lit.captures_iter(caps.get(1).map_or("", |m| m.as_str())) {
            let name = l
                .get(1)
                .or_else(|| l.get(2))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            if !name.is_empty() && !deps.contains(&name) {
                deps.push(name);
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模板名的不变量：**清洗之后结果必须还在模板目录里**。
    ///
    /// 注意断言的是「落在里面」而不是「被拒绝」——`..` 是被剔掉而不是报错，
    /// MCP 侧那条 `template_writes_cannot_escape_the_template_dir` 明确认可这个语义。
    /// 只有拼不出名字（空）或真的换掉了整条路径（Windows 盘符）才报错。
    #[test]
    fn resolve_template_never_escapes_the_template_dir() {
        let root = Path::new("/site/templates");
        for hostile in [
            "../evil.html",
            r"..\evil.html",
            "../../evil.html",
            "./x.html",
        ] {
            let path = resolve_template(root, hostile)
                .unwrap_or_else(|e| panic!("{hostile} 不该报错：{e}"));
            assert!(
                path.starts_with(root),
                "{hostile} 解析成了 {path:?}，越出模板目录"
            );
        }
    }

    #[test]
    fn resolve_template_rejects_unusable_names() {
        let root = Path::new("/site/templates");
        // 清洗后什么都不剩，无处可写。
        for bad in ["", "..", "./.."] {
            assert!(
                resolve_template(root, bad).is_err(),
                "{bad} 应当被拒绝，但通过了"
            );
        }
    }

    #[test]
    fn resolve_template_keeps_normal_names() {
        let root = Path::new("/site/templates");
        assert_eq!(
            resolve_template(root, "components/header.html").unwrap(),
            root.join("components").join("header.html")
        );
        assert_eq!(
            resolve_template(root, r"pages\post.html").unwrap(),
            root.join("pages").join("post.html")
        );
    }

    #[test]
    fn extracts_extends_include_and_import() {
        let src = r#"
{% extends "layouts/base.html" %}
{% block main %}
  {% include "components/pagination.html" %}
  {%- import "macros/i18n.html" as i18n %}
{% endblock %}
"#;
        let deps = extract_dependencies(src);
        assert_eq!(
            deps,
            vec![
                "layouts/base.html",
                "components/pagination.html",
                "macros/i18n.html"
            ]
        );
    }

    #[test]
    fn extracts_include_with_fallback_list() {
        let deps = extract_dependencies(r#"{% include ["a.html", 'b.html'] ignore missing %}"#);
        assert_eq!(deps, vec!["a.html", "b.html"]);
    }

    #[test]
    fn ignores_dynamic_include_expressions() {
        assert!(extract_dependencies("{% include page.template %}").is_empty());
    }

    #[test]
    fn kind_is_derived_from_directory() {
        assert_eq!(
            TemplateKind::from_name("components/header.html"),
            TemplateKind::Component
        );
        assert_eq!(
            TemplateKind::from_name("layouts/base.html"),
            TemplateKind::Layout
        );
        assert_eq!(
            TemplateKind::from_name("pages/post.html"),
            TemplateKind::Page
        );
        assert_eq!(
            TemplateKind::from_name("macros/i18n.html"),
            TemplateKind::Partial
        );
    }

    #[test]
    fn loads_templates_and_builds_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("layouts")).unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();
        std::fs::create_dir_all(root.join("pages")).unwrap();
        std::fs::write(
            root.join("layouts/base.html"),
            "<html><body>{% include \"components/header.html\" %}{% block main %}{% endblock %}</body></html>",
        )
        .unwrap();
        std::fs::write(root.join("components/header.html"), "<header>H</header>").unwrap();
        std::fs::write(
            root.join("pages/post.html"),
            "{% extends \"layouts/base.html\" %}{% block main %}{{ page.title }}{% endblock %}",
        )
        .unwrap();

        let set = TemplateSet::load(root).unwrap();
        assert_eq!(set.components().len(), 1);
        assert_eq!(set.layouts().len(), 1);
        let affected = set.graph.affected_templates(["components/header.html"]);
        assert!(affected.contains("pages/post.html"));

        let mut known = set.hashes();
        assert!(set.changed_since(&known).is_empty());
        known.insert("components/header.html".into(), "stale".into());
        assert_eq!(set.changed_since(&known), vec!["components/header.html"]);
    }

    #[test]
    fn missing_template_dir_is_reported() {
        let err = TemplateSet::load(Path::new("/definitely/not/here"))
            .err()
            .expect("目录不存在时应当报错");
        assert!(matches!(err, Error::InvalidProject(_)));
    }
}
