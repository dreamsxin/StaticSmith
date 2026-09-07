//! front matter 的就地读写。
//!
//! 编辑器编辑的是整份源文（含 `+++` 围栏），但「标题、日期、标签、草稿」这类字段
//! 用表单填远比手写 TOML 顺手。难点在于改完不能把用户的其他键、注释、书写风格洗掉，
//! 所以这里用 `toml_edit` 做保序改写，而不是 `toml::Value` 往返序列化。
//!
//! 输入输出都是字符串、不碰磁盘：调用方（界面）仍持有唯一的源文真相，
//! 表单只是把改动折算成新的源文，不会出现「表单值」与「源文」两份状态互相打架。

use toml_edit::{value, Array, DocumentMut, Item, Value};

use crate::content::FrontMatter;
use crate::error::{Error, Result};

/// front matter 围栏（与 [`crate::content`] 保持一致）。
const FENCE: &str = "+++";

/// 表单能改的字段。`None` 表示这一项不动。
///
/// 字符串给空串、数组给空表示「删掉这个键」——留下 `description = ""` 这种空值
/// 只会让源文越写越长，而语义与不写完全相同。
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Patch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>,
    pub template: Option<String>,
    pub slug: Option<String>,
    pub tags: Option<Vec<String>>,
    /// SEO 关键词。留空数组即删键，页面随之回退到用 tags 当关键词。
    pub keywords: Option<Vec<String>>,
    /// 旧地址。改 slug 时把老地址加进来，构建会为它生成重定向页。
    pub aliases: Option<Vec<String>>,
    pub draft: Option<bool>,
    pub weight: Option<i64>,
}

/// 往 `aliases` 里追加一个旧地址。已经写过就返回 `None`（不必重写文件）。
///
/// 搬动文章与栏目改名都要做这件事，规则只能有一份：`aliases` 是有序的，
/// 追加而不是排序，好让最近一次改名留在最后，看源文时能读出搬迁顺序。
pub fn push_alias(raw: &str, url: &str) -> Result<Option<String>> {
    let existing = read(raw)?.aliases;
    if existing.iter().any(|a| a == url) {
        return Ok(None);
    }
    let mut aliases = existing;
    aliases.push(url.to_string());
    let updated = apply(
        raw,
        &Patch {
            aliases: Some(aliases),
            ..Patch::default()
        },
    )?;
    Ok(Some(updated))
}

/// 读出 front matter 字段，供表单回填。没有围栏时返回默认值而不是报错——
/// 手写的内容文件可以完全没有 front matter。
pub fn read(raw: &str) -> Result<FrontMatter> {
    let (fm, _) = split(raw)?;
    toml::from_str(&fm).map_err(|e| Error::Other(format!("front matter 解析失败: {e}")))
}

/// 把表单改动折算成新的源文。正文原样保留，未涉及的键、注释与顺序都不动。
pub fn apply(raw: &str, patch: &Patch) -> Result<String> {
    let (fm, body) = split(raw)?;
    let mut doc: DocumentMut = fm
        .parse()
        .map_err(|e| Error::Other(format!("front matter 解析失败: {e}")))?;
    let table = doc.as_table_mut();

    if let Some(title) = &patch.title {
        set_or_remove(table, "title", title.is_empty(), || value(title));
    }
    if let Some(description) = &patch.description {
        set_or_remove(table, "description", description.is_empty(), || {
            value(description)
        });
    }
    if let Some(date) = &patch.date {
        set_or_remove(table, "date", date.is_empty(), || value(date));
    }
    if let Some(template) = &patch.template {
        set_or_remove(table, "template", template.is_empty(), || value(template));
    }
    if let Some(slug) = &patch.slug {
        set_or_remove(table, "slug", slug.is_empty(), || value(slug));
    }
    if let Some(tags) = &patch.tags {
        set_string_array(table, "tags", tags);
    }
    if let Some(keywords) = &patch.keywords {
        set_string_array(table, "keywords", keywords);
    }
    if let Some(aliases) = &patch.aliases {
        set_string_array(table, "aliases", aliases);
    }
    if let Some(draft) = patch.draft {
        // draft = false 与不写等价，删掉更干净（新建内容的骨架也是这个约定）。
        set_or_remove(table, "draft", !draft, || value(true));
    }
    if let Some(weight) = patch.weight {
        set_or_remove(table, "weight", weight == 0, || value(weight));
    }

    Ok(compose(&doc.to_string(), &body))
}

/// 设置或删除一个键。`remove` 为真时删除，否则用 `make` 造出新值。
fn set_or_remove(
    table: &mut toml_edit::Table,
    key: &str,
    remove: bool,
    make: impl FnOnce() -> Item,
) {
    if remove {
        table.remove(key);
    } else {
        table[key] = make();
    }
}

/// 写入字符串数组：逐项去空白、丢掉空项，全空即删键。
fn set_string_array(table: &mut toml_edit::Table, key: &str, values: &[String]) {
    let cleaned: Vec<&str> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    set_or_remove(table, key, cleaned.is_empty(), || {
        let mut array = Array::new();
        for item in &cleaned {
            array.push(*item);
        }
        Item::Value(Value::Array(array))
    });
}

/// 拆出 front matter 文本与正文。没有围栏时 front matter 为空串。
fn split(raw: &str) -> Result<(String, String)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix(FENCE) else {
        return Ok((String::new(), trimmed.to_string()));
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let Some(end) = rest.find(FENCE) else {
        return Err(Error::Other(
            "front matter 缺少结束的 `+++`，请先补齐再用属性面板".to_string(),
        ));
    };
    let (fm, body) = rest.split_at(end);
    let body = body[FENCE.len()..].trim_start_matches(['\r', '\n']);
    Ok((fm.to_string(), body.to_string()))
}

/// 拼回源文：围栏内恰好一个换行结尾，围栏后空一行接正文。
fn compose(fm: &str, body: &str) -> String {
    let fm = fm.trim_end_matches(['\n', '\r']);
    let mut out = String::from(FENCE);
    out.push('\n');
    if !fm.is_empty() {
        out.push_str(fm);
        out.push('\n');
    }
    out.push_str(FENCE);
    out.push_str("\n\n");
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "+++\ntitle = \"原标题\"\n# 这行注释不该丢\ndate = \"2026-01-02\"\ntags = [\"模板\"]\n\n[extra]\nauthor = \"张三\"\n+++\n\n正文第一行\n\n正文第二行\n";

    fn patch_title(title: &str) -> Patch {
        Patch {
            title: Some(title.to_string()),
            ..Patch::default()
        }
    }

    #[test]
    fn reads_fields_for_the_form() {
        let fm = read(SAMPLE).unwrap();
        assert_eq!(fm.title, "原标题");
        assert_eq!(fm.date.as_deref(), Some("2026-01-02"));
        assert_eq!(fm.tags, vec!["模板".to_string()]);
        assert!(!fm.draft);
    }

    #[test]
    fn missing_front_matter_reads_as_empty() {
        let fm = read("# 只有正文\n").unwrap();
        assert_eq!(fm.title, "");
        assert!(fm.tags.is_empty());
    }

    #[test]
    fn unterminated_front_matter_is_reported() {
        let err = apply("+++\ntitle = \"x\"\n", &patch_title("y")).unwrap_err();
        assert!(err.to_string().contains("缺少结束"));
    }

    #[test]
    fn editing_one_field_keeps_comments_other_keys_and_body() {
        let updated = apply(SAMPLE, &patch_title("新标题")).unwrap();
        assert!(updated.contains("title = \"新标题\""));
        assert!(!updated.contains("原标题"));
        assert!(
            updated.contains("# 这行注释不该丢"),
            "注释被抹掉了：{updated}"
        );
        assert!(updated.contains("author = \"张三\""));
        assert!(updated.contains("正文第一行\n\n正文第二行\n"));
    }

    #[test]
    fn clearing_a_field_removes_the_key() {
        let updated = apply(
            SAMPLE,
            &Patch {
                date: Some(String::new()),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(!updated.contains("date ="), "空值应当删键：{updated}");
    }

    #[test]
    fn tags_round_trip_with_cjk_and_trimming() {
        let updated = apply(
            SAMPLE,
            &Patch {
                tags: Some(vec![" 增量构建 ".into(), "模板".into(), "  ".into()]),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(
            updated.contains("tags = [\"增量构建\", \"模板\"]"),
            "{updated}"
        );
        // 空白项被丢掉，不会生成 tags = ["", ...] 这种脏数据
        assert_eq!(read(&updated).unwrap().tags.len(), 2);
    }

    #[test]
    fn aliases_can_be_added_and_removed() {
        let updated = apply(
            SAMPLE,
            &Patch {
                aliases: Some(vec!["/posts/old-slug/".into(), " ".into()]),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(
            updated.contains("aliases = [\"/posts/old-slug/\"]"),
            "{updated}"
        );

        // 空数组即删键：不留 aliases = [] 这种噪音
        let cleared = apply(
            &updated,
            &Patch {
                aliases: Some(Vec::new()),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(!cleared.contains("aliases"), "{cleared}");
    }

    #[test]
    fn keywords_are_written_next_to_tags() {
        let updated = apply(
            SAMPLE,
            &Patch {
                keywords: Some(vec!["静态站点".into(), " 增量构建 ".into()]),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(
            updated.contains("keywords = [\"静态站点\", \"增量构建\"]"),
            "{updated}"
        );
        // 标签没被动过
        assert!(updated.contains("tags = [\"模板\"]"));
        assert_eq!(read(&updated).unwrap().keywords.len(), 2);
    }

    #[test]
    fn draft_is_written_only_when_true() {
        let on = apply(
            SAMPLE,
            &Patch {
                draft: Some(true),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(on.contains("draft = true"));

        let off = apply(
            &on,
            &Patch {
                draft: Some(false),
                ..Patch::default()
            },
        )
        .unwrap();
        assert!(!off.contains("draft"), "draft = false 与不写等价：{off}");
    }

    #[test]
    fn front_matter_is_created_when_absent() {
        let updated = apply("# 只有正文\n", &patch_title("补上的标题")).unwrap();
        assert!(updated.starts_with("+++\ntitle = \"补上的标题\"\n+++\n\n"));
        assert!(updated.ends_with("# 只有正文\n"));
        assert_eq!(read(&updated).unwrap().title, "补上的标题");
    }

    #[test]
    fn special_characters_are_escaped_not_broken() {
        let updated = apply(SAMPLE, &patch_title(r#"带 "引号" 与 \ 反斜杠"#)).unwrap();
        // 关键是能被重新解析回来，而不是长成某个特定写法
        assert_eq!(read(&updated).unwrap().title, r#"带 "引号" 与 \ 反斜杠"#);
    }

    #[test]
    fn applying_the_same_patch_twice_is_stable() {
        let once = apply(SAMPLE, &patch_title("稳定")).unwrap();
        let twice = apply(&once, &patch_title("稳定")).unwrap();
        assert_eq!(once, twice, "重复写入不该产生额外空行或位移");
    }
}
