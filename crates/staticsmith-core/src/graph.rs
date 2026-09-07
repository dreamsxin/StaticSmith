use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

/// 模板依赖图。
///
/// 边的方向是「模板 → 它直接引用的模板」（`extends` / `include` / `import`）。
/// 级联更新需要反向查询：某个组件变更后，哪些模板受影响。
#[derive(Debug, Clone, Default, Serialize)]
pub struct TemplateGraph {
    /// 模板 → 直接引用的模板集合。
    uses: BTreeMap<String, BTreeSet<String>>,
    /// 模板 → 直接引用它的模板集合（反向边）。
    used_by: BTreeMap<String, BTreeSet<String>>,
}

impl TemplateGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个模板及其直接依赖。重复登记会覆盖旧的依赖关系。
    pub fn set_dependencies(&mut self, template: &str, deps: impl IntoIterator<Item = String>) {
        if let Some(old) = self.uses.remove(template) {
            for dep in old {
                if let Some(set) = self.used_by.get_mut(&dep) {
                    set.remove(template);
                }
            }
        }
        let deps: BTreeSet<String> = deps.into_iter().collect();
        for dep in &deps {
            self.used_by
                .entry(dep.clone())
                .or_default()
                .insert(template.to_string());
        }
        self.uses.insert(template.to_string(), deps);
    }

    pub fn templates(&self) -> impl Iterator<Item = &String> {
        self.uses.keys()
    }

    pub fn direct_dependencies(&self, template: &str) -> Vec<String> {
        self.uses
            .get(template)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn direct_dependents(&self, template: &str) -> Vec<String> {
        self.used_by
            .get(template)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 变更集的传递闭包：包含变更模板自身与所有（间接）引用它们的模板。
    ///
    /// 这是「修改 header.html 影响 32 个页面」提示的第一步。存在环时不会死循环。
    pub fn affected_templates<'a>(
        &self,
        changed: impl IntoIterator<Item = &'a str>,
    ) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        for c in changed {
            if seen.insert(c.to_string()) {
                queue.push_back(c.to_string());
            }
        }
        while let Some(current) = queue.pop_front() {
            if let Some(parents) = self.used_by.get(&current) {
                for p in parents {
                    if seen.insert(p.clone()) {
                        queue.push_back(p.clone());
                    }
                }
            }
        }
        seen
    }

    /// 模板自身依赖的传递闭包（向下），用于「布局继承预览」时确定需要加载的组件。
    pub fn transitive_dependencies(&self, template: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from(vec![template.to_string()]);
        while let Some(current) = queue.pop_front() {
            if let Some(deps) = self.uses.get(&current) {
                for d in deps {
                    if seen.insert(d.clone()) {
                        queue.push_back(d.clone());
                    }
                }
            }
        }
        seen
    }

    /// 组件树：供前端渲染「布局 → 组件」的可视化结构。
    pub fn tree(&self, root: &str) -> TemplateNode {
        self.build_node(root, &mut BTreeSet::new())
    }

    fn build_node(&self, template: &str, path: &mut BTreeSet<String>) -> TemplateNode {
        let cyclic = !path.insert(template.to_string());
        let children = if cyclic {
            Vec::new()
        } else {
            self.direct_dependencies(template)
                .into_iter()
                .map(|d| self.build_node(&d, path))
                .collect()
        };
        if !cyclic {
            path.remove(template);
        }
        TemplateNode {
            name: template.to_string(),
            cyclic,
            children,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateNode {
    pub name: String,
    /// 命中循环引用时为 true，前端据此提示模板写法有误。
    pub cyclic: bool,
    pub children: Vec<TemplateNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// base ← index / post ← ...；header 与 footer 被 base 引用。
    fn sample() -> TemplateGraph {
        let mut g = TemplateGraph::new();
        g.set_dependencies(
            "layouts/base.html",
            [
                "components/header.html".into(),
                "components/footer.html".into(),
            ],
        );
        g.set_dependencies(
            "pages/index.html",
            [
                "layouts/base.html".into(),
                "components/pagination.html".into(),
            ],
        );
        g.set_dependencies("pages/post.html", ["layouts/base.html".into()]);
        g.set_dependencies("components/header.html", []);
        g.set_dependencies("components/footer.html", []);
        g.set_dependencies("components/pagination.html", []);
        g
    }

    #[test]
    fn header_change_cascades_to_all_pages() {
        let g = sample();
        let affected = g.affected_templates(["components/header.html"]);
        assert!(affected.contains("layouts/base.html"));
        assert!(affected.contains("pages/index.html"));
        assert!(affected.contains("pages/post.html"));
        assert!(!affected.contains("components/pagination.html"));
    }

    #[test]
    fn pagination_change_only_hits_list_pages() {
        let g = sample();
        let affected = g.affected_templates(["components/pagination.html"]);
        assert!(affected.contains("pages/index.html"));
        assert!(!affected.contains("pages/post.html"));
    }

    #[test]
    fn resetting_dependencies_removes_stale_reverse_edges() {
        let mut g = sample();
        g.set_dependencies("pages/post.html", []);
        let affected = g.affected_templates(["layouts/base.html"]);
        assert!(!affected.contains("pages/post.html"));
        assert!(affected.contains("pages/index.html"));
    }

    #[test]
    fn transitive_dependencies_walk_down() {
        let g = sample();
        let deps = g.transitive_dependencies("pages/index.html");
        assert!(deps.contains("components/header.html"));
        assert!(deps.contains("layouts/base.html"));
    }

    #[test]
    fn cycles_do_not_hang() {
        let mut g = TemplateGraph::new();
        g.set_dependencies("a.html", ["b.html".into()]);
        g.set_dependencies("b.html", ["a.html".into()]);
        assert_eq!(g.affected_templates(["a.html"]).len(), 2);
        let tree = g.tree("a.html");
        assert!(tree.children[0].children[0].cyclic);
    }
}
