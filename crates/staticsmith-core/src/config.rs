use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// 项目根目录下的配置文件名。
pub const CONFIG_FILE_NAME: &str = "staticsmith.toml";

/// `staticsmith.toml` 的完整映射。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiteConfig {
    pub site: Site,
    #[serde(default)]
    pub build: Build,
    #[serde(default)]
    pub assets: Assets,
    #[serde(default)]
    pub taxonomy: Taxonomy,
    /// 额外的分类维度。写了它就以它为准，`[taxonomy]` 退化为不生效的旧写法。
    #[serde(default)]
    pub taxonomies: Vec<Taxonomy>,
    /// 导航菜单。空数组表示模板自己写死链接（老站点保持原样）。
    #[serde(default)]
    pub menu: Vec<MenuItem>,
    #[serde(default)]
    pub deploy: Deploy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Site {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_language")]
    pub language: String,
    /// 任意扩展字段，原样透传给模板的 `site.extra`。
    #[serde(default)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    #[serde(default = "default_content_dir")]
    pub content_dir: PathBuf,
    #[serde(default = "default_theme_dir")]
    pub theme_dir: PathBuf,
    /// 统一模板目录（layouts / components / pages）。
    #[serde(default = "default_template_dir")]
    pub template_dir: PathBuf,
    /// 站点级静态资源目录，整体复制到输出目录根部。
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub minify: bool,
    /// 生成 `sitemap.xml`（需要 `site.base_url`）。
    #[serde(default = "default_true")]
    pub generate_sitemap: bool,
    /// 生成 `feed.xml`（Atom，需要 `site.base_url`）。
    #[serde(default = "default_true")]
    pub generate_feed: bool,
    /// 订阅条目数上限，0 表示不限制。
    #[serde(default = "default_feed_limit")]
    pub feed_limit: usize,
    /// 是否发布 `date` 晚于构建时刻的文章。
    ///
    /// 默认 `true`，保持「写什么日期都会发出去」的直觉。改成 `false` 就得到定时发布：
    /// 未来日期的文章暂不进产物，等构建时刻越过那个日期才出现——配合定时 CI 构建，
    /// 就是「排好队，到点上线」。
    #[serde(default = "default_true")]
    pub publish_future: bool,
    /// 正文按哪种格式解析。
    ///
    /// 全站统一，不做每篇覆盖：一个站点里两种正文格式混排，模板、体检、导入
    /// 每一处都要问「这一篇是哪种」，而收益只是省掉一次目录划分。
    #[serde(default)]
    pub source_format: SourceFormat,
}

/// 正文的源码格式。
///
/// `Markdown` 是默认：`**加粗**` 会被渲染。`Html` 则**原样输出**——正文里写什么标签
/// 就是什么标签，`**` 就是两个星号。刻意不做「HTML 里仍然跑一遍 Markdown」那种混合模式：
/// 混合模式下「这段为什么被转义了」永远解释不清，而想混写的人本来就可以在 Markdown 里
/// 直接写 HTML 块（Markdown 模式已经支持）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    #[default]
    Markdown,
    Html,
}

/// 编辑器插入的图片等媒体资源如何落盘。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assets {
    /// 相对 `build.static_dir` 的子目录。
    ///
    /// 刻意限定为相对路径：资源目录必须位于 static_dir 之内，
    /// 这样「复制到产物」与「页面里的 URL」由同一份路径推导出来，不可能对不上。
    #[serde(default = "default_assets_dir")]
    pub dir: String,
    /// 文件命名策略。
    #[serde(default)]
    pub naming: AssetNaming,
    /// 哈希截断长度（十六进制字符数），8..=64。
    #[serde(default = "default_hash_length")]
    pub hash_length: usize,
    /// 是否用哈希前两位做二级目录，避免单目录堆积上万文件。
    #[serde(default = "default_true")]
    pub shard: bool,
    /// 单个文件大小上限（MB），0 表示不限制。
    #[serde(default = "default_max_asset_mb")]
    pub max_size_mb: u64,
    /// URL 前缀覆盖。留空时按 `dir` 推导，例如 `images` → `/images/`。
    #[serde(default)]
    pub url_prefix: Option<String>,
}

/// 资源文件名的生成方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetNaming {
    /// SHA-256 内容哈希（默认）。同一张图重复粘贴只保存一份。
    #[default]
    Sha256,
    /// MD5 内容哈希。仅用于与既有站点资源命名保持一致，不用于安全场景。
    Md5,
    /// 保留原文件名（清洗后）；重名且内容不同时追加短哈希。
    Original,
}

impl Default for Assets {
    fn default() -> Self {
        Self {
            dir: default_assets_dir(),
            naming: AssetNaming::default(),
            hash_length: default_hash_length(),
            shard: true,
            max_size_mb: default_max_asset_mb(),
            url_prefix: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deploy {
    /// `git` 或 `ftp`。
    #[serde(default)]
    pub r#type: DeployKind,
    #[serde(default)]
    pub git: Option<GitDeploy>,
    #[serde(default)]
    pub ftp: Option<FtpDeploy>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeployKind {
    #[default]
    None,
    Git,
    Ftp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDeploy {
    pub remote: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default = "default_commit_message")]
    pub commit_message: String,
    /// `token` 或 `ssh`。凭证本身不落盘，见 `docs/deploy.md`。
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub ssh_key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpDeploy {
    pub host: String,
    #[serde(default = "default_ftp_port")]
    pub port: u16,
    pub username: String,
    /// 只保存环境变量名；真实密码存放于系统凭据管理器。
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default = "default_remote_path")]
    pub remote_path: String,
    /// **暂不支持**，必须为 false，`validate()` 会拦下 true。
    ///
    /// 字段保留是为了让写过 `sftp = true` 的老配置仍能解析——直接删字段的话 serde 会
    /// 静默忽略它，用户会以为 SFTP 开着。要支持它得开 deploy 的 `sftp` feature，
    /// 那会把 `ssh2` / `libssh2-sys` 打进产物。
    #[serde(default)]
    pub sftp: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            content_dir: default_content_dir(),
            theme_dir: default_theme_dir(),
            template_dir: default_template_dir(),
            static_dir: default_static_dir(),
            page_size: default_page_size(),
            minify: false,
            generate_sitemap: true,
            generate_feed: true,
            feed_limit: default_feed_limit(),
            publish_future: true,
            source_format: SourceFormat::Markdown,
        }
    }
}

/// 一个 taxonomy（分类维度）的页面生成规则。
///
/// front matter 里的同名字段由此变成可浏览的两级页面：总览与单个词条。
/// `tags` 是默认那一个；再加「分类」等维度用 `[[taxonomies]]`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taxonomy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// front matter 里的字段名，如 `tags`、`categories`。留空按 `slug` 推。
    #[serde(default = "default_taxonomy_slug")]
    pub name: String,
    /// URL 前缀，如 `tags` → `/tags/` 与 `/tags/rust/`。
    #[serde(default = "default_taxonomy_slug")]
    pub slug: String,
    /// 总览页标题。
    #[serde(default = "default_taxonomy_title")]
    pub title: String,
    /// 总览页与单标签页模板。缺失时跳过生成并在构建报告里给出提示。
    #[serde(default = "default_taxonomy_list_template")]
    pub list_template: String,
    #[serde(default = "default_taxonomy_term_template")]
    pub term_template: String,
}

impl Default for Taxonomy {
    fn default() -> Self {
        Self {
            enabled: true,
            name: default_taxonomy_slug(),
            slug: default_taxonomy_slug(),
            title: default_taxonomy_title(),
            list_template: default_taxonomy_list_template(),
            term_template: default_taxonomy_term_template(),
        }
    }
}

impl Taxonomy {
    /// 规范化后的 URL 前缀（去掉首尾斜杠）。
    pub fn normalized_slug(&self) -> String {
        self.slug.replace('\\', "/").trim_matches('/').to_string()
    }

    /// 读取哪个 front matter 字段。留空退回 slug——多数站点两者同名。
    pub fn field(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.normalized_slug()
        } else {
            name.to_string()
        }
    }

    fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if !self.enabled {
            return issues;
        }
        let slug = self.normalized_slug();
        if slug.is_empty() {
            issues.push("taxonomy.slug 不能为空".to_string());
        }
        if slug.split('/').any(|s| s == "..") {
            issues.push("taxonomy.slug 不能包含 `..`".to_string());
        }
        if self.field().is_empty() {
            issues.push("taxonomy.name 与 slug 不能同时为空".to_string());
        }
        issues
    }
}

/// 导航菜单里的一项。
///
/// 菜单刻意做成**配置**而不是内容：它回答的是「这个站怎么被逛」，
/// 与某一篇文章无关。放进 `staticsmith.toml`，模板只负责遍历，
/// 于是加栏目不用再改 `components/header.html`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuItem {
    /// 菜单上显示的文字。
    pub name: String,
    /// 目标地址：站内写 `/posts/`，站外写完整 URL。
    pub url: String,
    /// 排序权重，小的在前；相同权重保持书写顺序。
    #[serde(default)]
    pub weight: i32,
    /// 新窗口打开。站外链接常用，站内一般不需要。
    #[serde(default)]
    pub blank: bool,
}

impl MenuItem {
    /// 是否指向站外。判断只看协议前缀，模板据此决定要不要加 `rel="noopener"`。
    pub fn is_external(&self) -> bool {
        let url = self.url.trim();
        url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("//")
            || url.starts_with("mailto:")
    }

    fn validate(&self, index: usize) -> Vec<String> {
        let mut issues = Vec::new();
        if self.name.trim().is_empty() {
            issues.push(format!("menu[{index}].name 不能为空"));
        }
        let url = self.url.trim();
        if url.is_empty() {
            issues.push(format!("menu[{index}].url 不能为空"));
        } else if !self.is_external() && !url.starts_with('/') && !url.starts_with('#') {
            // 相对地址在不同深度的页面上会解析到不同目标，菜单是全站共用的，必须写绝对路径
            issues.push(format!("menu[{index}].url 站内地址要以 `/` 开头"));
        }
        // 模板把地址原样写进 href（不转义，否则 `/` 会变成 &#x2F;），
        // 所以这些字符必须在入口拦掉，不能让配置写出属性逃逸
        if url.contains(['"', '\'', '<', '>']) {
            issues.push(format!("menu[{index}].url 不能包含引号或尖括号"));
        }
        issues
    }
}

impl Assets {
    /// 资源目录的规范化形式：去掉首尾斜杠，统一正斜杠。
    pub fn normalized_dir(&self) -> String {
        self.dir.replace('\\', "/").trim_matches('/').to_string()
    }

    /// 站内 URL 前缀，形如 `/images/`。
    pub fn url_prefix(&self) -> String {
        if let Some(prefix) = &self.url_prefix {
            let trimmed = prefix.trim_end_matches('/');
            return format!("{}/", if trimmed.is_empty() { "" } else { trimmed });
        }
        let dir = self.normalized_dir();
        if dir.is_empty() {
            "/".to_string()
        } else {
            format!("/{dir}/")
        }
    }

    /// 哈希截断长度，夹在 8..=64 之间避免配置写错导致文件名碰撞或过长。
    pub fn effective_hash_length(&self) -> usize {
        self.hash_length.clamp(8, 64)
    }

    fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let dir = self.normalized_dir();
        if dir.split('/').any(|s| s == "..") {
            issues.push("assets.dir 不能包含 `..`".to_string());
        }
        if Path::new(&self.dir).is_absolute() {
            issues.push("assets.dir 必须是相对 build.static_dir 的路径".to_string());
        }
        if !(8..=64).contains(&self.hash_length) {
            issues.push("assets.hash_length 必须在 8..=64 之间".to_string());
        }
        issues
    }
}

/// 把 `source` 里的键值合并进 `target`，保住 `target` 的注释、键序与多余的键。
///
/// - 两边都是表就往下递归，这样表内的注释与键序都留着
/// - 改值时**只换值、不换键**：`Table::insert` 会连键一起替换，而键前面那行注释
///   （`# 首页大标题`）正是挂在键上的，换掉就丢了。值自己的 decor（行尾注释）也照抄回去
/// - 数组表（`[[taxonomies]]` / `[[menu]]`）整块替换：逐项对齐要先定义「哪一项是同一项」，
///   而条目可增删改序，猜错了比整块换掉更糟。代价是数组表内部的注释会丢，
///   这一条写进了 `docs/configuration.md`
/// - `target` 里我们不认识的键一个不动：用户可能在配置里放了给别的工具看的段
fn merge_table(target: &mut toml_edit::Table, source: &toml_edit::Table) {
    use toml_edit::Item;

    for (key, value) in source.iter() {
        match (target.get_mut(key), value) {
            (Some(Item::Table(existing)), Item::Table(fresh)) => merge_table(existing, fresh),
            (Some(slot), _) => {
                let mut fresh = value.clone();
                if let (Some(old), Some(new)) = (slot.as_value(), fresh.as_value_mut()) {
                    *new.decor_mut() = old.decor().clone();
                }
                *slot = fresh;
            }
            (None, _) => {
                target.insert(key, value.clone());
            }
        }
    }
}

impl SiteConfig {
    /// 从项目根目录读取 `staticsmith.toml`。
    pub fn load(project_root: impl AsRef<Path>) -> Result<Self> {
        let path = project_root.as_ref().join(CONFIG_FILE_NAME);
        let raw = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        Self::parse_at(&path, &raw)
    }

    /// 解析一份配置文本（源码视图保存前要先过这一道）。
    ///
    /// 与 [`Self::load`] 共用同一份解析：两处各写一次 `toml::from_str`，
    /// 迟早出现「界面说这份配置能用、打开站点却报错」。
    pub fn parse(raw: &str) -> Result<Self> {
        Self::parse_at(Path::new(CONFIG_FILE_NAME), raw)
    }

    fn parse_at(path: &Path, raw: &str) -> Result<Self> {
        toml::from_str(raw).map_err(|source| Error::config_parse(path.to_path_buf(), source))
    }

    /// 写回 `staticsmith.toml`（可视化界面保存设置时调用）。
    ///
    /// **保序改写**，不是整文件重新序列化：后者会把用户写在配置里的注释与
    /// 我们不认识的键一并抹掉——设置页保存一次，`# 这台机器上别开 minify`
    /// 这类说明就没了，而配置文件恰恰是最值得写注释的地方。
    /// 与 front matter 用同一套办法（`toml_edit`），理由也同一条。
    pub fn save(&self, project_root: impl AsRef<Path>) -> Result<()> {
        let path = project_root.as_ref().join(CONFIG_FILE_NAME);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let merged = self.merge_into(&existing)?;
        std::fs::write(&path, merged).map_err(|e| Error::io(&path, e))
    }

    /// 把当前配置合并进一份既有的 TOML 文本，返回新文本。
    ///
    /// 单独抽出来是为了能直接测「注释还在不在」：涉及磁盘的话，
    /// 这类断言就得先摆一个临时目录。
    pub fn merge_into(&self, existing: &str) -> Result<String> {
        use toml_edit::DocumentMut;

        let fresh: DocumentMut = toml::to_string(self)?
            .parse()
            .map_err(|e: toml_edit::TomlError| Error::Other(format!("配置序列化失败: {e}")))?;
        if existing.trim().is_empty() {
            return Ok(fresh.to_string());
        }
        let mut doc: DocumentMut = existing
            .parse()
            .map_err(|e: toml_edit::TomlError| Error::Other(format!("配置解析失败: {e}")))?;
        merge_table(doc.as_table_mut(), fresh.as_table());
        Ok(doc.to_string())
    }

    /// 生效的分类维度。
    ///
    /// 写了 `[[taxonomies]]` 就以它为准；否则把旧的 `[taxonomy]` 当成单个 tags 维度。
    /// 只返回启用的项，调用方不必再判 `enabled`。
    pub fn effective_taxonomies(&self) -> Vec<Taxonomy> {
        if self.taxonomies.is_empty() {
            if self.taxonomy.enabled {
                return vec![self.taxonomy.clone()];
            }
            return Vec::new();
        }
        self.taxonomies
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect()
    }

    /// 排好序的导航菜单。
    ///
    /// 权重小的在前，权重相同保持书写顺序（`sort_by_key` 是稳定排序），
    /// 于是「都不填 weight」等于「按写的顺序显示」——最符合直觉的默认。
    pub fn menu_items(&self) -> Vec<MenuItem> {
        let mut items = self.menu.clone();
        items.sort_by_key(|item| item.weight);
        items
    }

    /// 校验必填项，返回人类可读的问题列表（空列表表示配置合法）。
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.site.title.trim().is_empty() {
            issues.push("site.title 不能为空".to_string());
        }
        if self.build.page_size == 0 {
            issues.push("build.page_size 必须大于 0".to_string());
        }
        issues.extend(self.assets.validate());
        for taxonomy in self.effective_taxonomies() {
            issues.extend(taxonomy.validate());
        }
        // 两个维度共用一个 URL 前缀会互相覆盖产物，必须拦下来
        let mut slugs: Vec<String> = self
            .effective_taxonomies()
            .iter()
            .map(|t| t.normalized_slug())
            .collect();
        slugs.sort();
        if slugs.windows(2).any(|w| w[0] == w[1]) {
            issues.push("taxonomies 里出现了重复的 slug，产物会互相覆盖".to_string());
        }
        for (index, item) in self.menu.iter().enumerate() {
            issues.extend(item.validate(index));
        }
        match self.deploy.r#type {
            DeployKind::Git if self.deploy.git.is_none() => {
                issues.push("deploy.type = \"git\" 但缺少 [deploy.git] 配置段".to_string());
            }
            DeployKind::Ftp if self.deploy.ftp.is_none() => {
                issues.push("deploy.type = \"ftp\" 但缺少 [deploy.ftp] 配置段".to_string());
            }
            _ => {}
        }
        // SFTP 暂不支持：字段还留着（老配置照旧能解析），但在这里就拦下来。
        // 不拦的话用户要到点「发布」时才撞上「当前构建未启用 sftp 特性」——
        // 一个能填、能存、只在最后一步失败的开关比没有这个开关更糟。
        if let Some(ftp) = &self.deploy.ftp {
            if ftp.sftp {
                issues.push(
                    "deploy.ftp.sftp 暂不支持，请设为 false（改用 Git 发布，或先本地生成再自行上传）"
                        .to_string(),
                );
            }
        }
        issues
    }
}

/// 项目路径集合：把配置中的相对路径统一解析为绝对路径。
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub content: PathBuf,
    pub templates: PathBuf,
    pub theme: PathBuf,
    pub output: PathBuf,
    /// 站点级静态资源目录。
    pub static_dir: PathBuf,
    /// 编辑器插入的媒体资源目录（位于 `static_dir` 之内）。
    pub assets: PathBuf,
    /// SQLite 索引文件位置：`<root>/.staticsmith/index.db`。
    pub index_db: PathBuf,
}

impl ProjectPaths {
    pub fn new(root: impl AsRef<Path>, build: &Build, assets: &Assets) -> Self {
        let root = tidy(root.as_ref());
        let join = |p: &Path| {
            if p.is_absolute() {
                tidy(p)
            } else {
                tidy(&root.join(p))
            }
        };
        let static_dir = join(&build.static_dir);
        let assets_dir = assets
            .normalized_dir()
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(static_dir.clone(), |acc, segment| acc.join(segment));
        Self {
            content: join(&build.content_dir),
            templates: join(&build.template_dir),
            theme: join(&build.theme_dir),
            output: join(&build.output_dir),
            assets: assets_dir,
            static_dir,
            index_db: root.join(".staticsmith").join("index.db"),
            root,
        }
    }
}

/// 把 `.` 段折掉。
///
/// 配置里的目录默认写成 `./content`，直接 `root.join()` 会留下
/// `D:\site\.\content` 这种路径——能打开，但显示给人看（`site_info`、错误信息、
/// 日志）就很扎眼。`Components` 迭代本身会跳过中间的 `.`，重新收集一遍即可。
fn tidy(path: &Path) -> PathBuf {
    path.components().collect()
}

fn default_language() -> String {
    "zh-CN".to_string()
}
fn default_output_dir() -> PathBuf {
    PathBuf::from("./dist")
}
fn default_content_dir() -> PathBuf {
    PathBuf::from("./content")
}
fn default_theme_dir() -> PathBuf {
    PathBuf::from("./themes/default")
}
fn default_template_dir() -> PathBuf {
    PathBuf::from("./templates")
}
fn default_static_dir() -> PathBuf {
    PathBuf::from("./static")
}
fn default_assets_dir() -> String {
    "images".to_string()
}
fn default_hash_length() -> usize {
    16
}
fn default_feed_limit() -> usize {
    20
}
fn default_taxonomy_slug() -> String {
    "tags".to_string()
}
fn default_taxonomy_title() -> String {
    "标签".to_string()
}
fn default_taxonomy_list_template() -> String {
    "pages/tags.html".to_string()
}
fn default_taxonomy_term_template() -> String {
    "pages/tag.html".to_string()
}
fn default_max_asset_mb() -> u64 {
    32
}
fn default_true() -> bool {
    true
}
fn default_page_size() -> usize {
    10
}
fn default_branch() -> String {
    "main".to_string()
}
fn default_commit_message() -> String {
    "站点更新于 {{ now() }}".to_string()
}
fn default_auth_type() -> String {
    "token".to_string()
}
fn default_ftp_port() -> u16 {
    21
}
fn default_remote_path() -> String {
    "/public_html".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[site]
title = "我的 Rust 静态站"
description = "基于 Tauri 的可视化工具"
base_url = "https://example.com"
language = "zh-CN"

[build]
output_dir = "./dist"
content_dir = "./content"
theme_dir = "./themes/default"
template_dir = "./templates"
page_size = 10
minify = true

[deploy]
type = "git"

[deploy.git]
remote = "https://github.com/user/repo.git"
branch = "main"
commit_message = "站点更新于 {{ now() }}"
auth_type = "token"
"#;

    #[test]
    fn parses_documented_config() {
        let cfg: SiteConfig = toml::from_str(SAMPLE).unwrap();
        assert_eq!(cfg.site.title, "我的 Rust 静态站");
        assert!(cfg.build.minify);
        assert_eq!(cfg.deploy.r#type, DeployKind::Git);
        assert_eq!(cfg.deploy.git.as_ref().unwrap().branch, "main");
        assert!(cfg.validate().is_empty());
    }

    /// 保存设置不能把注释与我们不认识的键洗掉。
    ///
    /// 这曾经是真的：`save` 整文件重新序列化，用户写的每条注释保存一次就没了。
    #[test]
    fn saving_keeps_comments_and_unknown_keys() {
        let existing = r#"# 站点信息
[site]
# 首页大标题
title = "旧标题"
base_url = "https://example.com"

[build]
page_size = 10
# 这台机器上别开 minify，调试时看不清
minify = false

# 给别的工具看的段，StaticSmith 不认识它
[my-tool]
enabled = true
"#;
        let mut cfg: SiteConfig = toml::from_str(existing).unwrap();
        cfg.site.title = "新标题".to_string();
        cfg.build.page_size = 20;

        let out = cfg.merge_into(existing).unwrap();
        assert!(out.contains("# 首页大标题"), "键上的注释要留着：{out}");
        assert!(out.contains("# 这台机器上别开 minify"), "{out}");
        assert!(out.contains("[my-tool]"), "不认识的段不能删：{out}");
        assert!(out.contains("enabled = true"), "{out}");
        assert!(out.contains("title = \"新标题\""), "值要真的改了：{out}");
        assert!(out.contains("page_size = 20"), "{out}");
        assert!(!out.contains("旧标题"), "{out}");
        // 合并出来的文本必须还能解析回配置，否则下一次打开站点就废了
        let reloaded: SiteConfig = toml::from_str(&out).unwrap();
        assert_eq!(reloaded.site.title, "新标题");
        assert_eq!(reloaded.build.page_size, 20);
        assert!(!reloaded.build.minify, "没改的键不能被默认值顶掉");
    }

    /// 空文件（或还没有配置文件）时退回整份写出，不该报错。
    #[test]
    fn saving_into_nothing_writes_a_full_config() {
        let cfg = SiteConfig::default();
        let out = cfg.merge_into("").unwrap();
        let reloaded: SiteConfig = toml::from_str(&out).unwrap();
        assert_eq!(reloaded.site.title, cfg.site.title);
    }

    #[test]
    fn defaults_fill_missing_build_section() {
        let cfg: SiteConfig = toml::from_str("[site]\ntitle = \"t\"\n").unwrap();
        assert_eq!(cfg.build.page_size, 10);
        assert_eq!(cfg.build.output_dir, PathBuf::from("./dist"));
        assert_eq!(cfg.deploy.r#type, DeployKind::None);
    }

    #[test]
    fn validate_reports_missing_deploy_section() {
        let cfg: SiteConfig =
            toml::from_str("[site]\ntitle = \"t\"\n\n[deploy]\ntype = \"ftp\"\n").unwrap();
        assert_eq!(cfg.validate().len(), 1);
    }

    /// `sftp = true` 要在保存时就被拦下，而不是等点了「发布」才失败。
    ///
    /// 同时确认字段本身仍能解析——写过它的老配置不该在打开站点时就报错。
    #[test]
    fn validate_rejects_unsupported_sftp() {
        let raw = "[site]\ntitle = \"t\"\n\n[deploy]\ntype = \"ftp\"\n\n\
                   [deploy.ftp]\nhost = \"h\"\nusername = \"u\"\nsftp = true\n";
        let cfg: SiteConfig = toml::from_str(raw).unwrap();

        let issues = cfg.validate();
        assert!(
            issues.iter().any(|i| i.contains("sftp")),
            "应当明确指出 sftp 不支持：{issues:?}"
        );
    }

    #[test]
    fn validate_accepts_ftp_without_sftp() {
        let raw = "[site]\ntitle = \"t\"\n\n[deploy]\ntype = \"ftp\"\n\n\
                   [deploy.ftp]\nhost = \"h\"\nusername = \"u\"\n";
        let cfg: SiteConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
    }

    #[test]
    fn menu_items_sort_by_weight_and_keep_written_order() {
        let cfg: SiteConfig = toml::from_str(
            r#"
[site]
title = "t"

[[menu]]
name = "关于"
url = "/about/"
weight = 9

[[menu]]
name = "文章"
url = "/posts/"

[[menu]]
name = "首页"
url = "/"
"#,
        )
        .unwrap();
        let names: Vec<String> = cfg.menu_items().into_iter().map(|item| item.name).collect();
        // 都没写 weight 的按原顺序，写了大 weight 的沉到最后
        assert_eq!(names, vec!["文章", "首页", "关于"]);
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn menu_rejects_empty_fields_and_relative_urls() {
        let cfg = SiteConfig {
            site: Site {
                title: "t".into(),
                ..Site::default()
            },
            menu: vec![
                MenuItem {
                    name: String::new(),
                    url: "/ok/".into(),
                    ..MenuItem::default()
                },
                MenuItem {
                    name: "相对地址".into(),
                    url: "posts/".into(),
                    ..MenuItem::default()
                },
            ],
            ..SiteConfig::default()
        };
        let issues = cfg.validate();
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].contains("menu[0].name"));
        assert!(issues[1].contains("menu[1].url"));
    }

    #[test]
    fn menu_external_detection_covers_protocols() {
        let external = |url: &str| {
            MenuItem {
                name: "x".into(),
                url: url.into(),
                ..MenuItem::default()
            }
            .is_external()
        };
        assert!(external("https://example.com"));
        assert!(external("//cdn.example.com/x"));
        assert!(external("mailto:me@example.com"));
        assert!(!external("/posts/"));
        assert!(!external("#top"));
    }

    #[test]
    fn menu_rejects_attribute_escaping_characters() {
        // 模板不转义地址（否则 `/` 会变成 &#x2F;），配置就得拦下能逃出 href="" 的字符
        let cfg = SiteConfig {
            site: Site {
                title: "t".into(),
                ..Site::default()
            },
            menu: vec![MenuItem {
                name: "坏地址".into(),
                url: "/x\" onmouseover=\"alert(1)".into(),
                ..MenuItem::default()
            }],
            ..SiteConfig::default()
        };
        let issues = cfg.validate();
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("引号"));
    }

    #[test]
    fn saved_config_keeps_the_menu() {
        // save() 是全量重写：菜单要是没进结构体，保存一次就没了
        let dir = tempfile::tempdir().unwrap();
        let cfg = SiteConfig {
            site: Site {
                title: "t".into(),
                ..Site::default()
            },
            menu: vec![MenuItem {
                name: "文章".into(),
                url: "/posts/".into(),
                weight: 2,
                blank: false,
            }],
            ..SiteConfig::default()
        };
        cfg.save(dir.path()).unwrap();
        let loaded = SiteConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.menu.len(), 1);
        assert_eq!(loaded.menu[0].url, "/posts/");
        assert_eq!(loaded.menu[0].weight, 2);
    }

    #[test]
    fn project_paths_resolve_relative_dirs() {
        let build = Build::default();
        let assets = Assets::default();
        let paths = ProjectPaths::new("/tmp/site", &build, &assets);
        // 配置默认写成 ./content，但落到路径上要干净——site_info、日志、错误信息都直接给人看
        assert_eq!(paths.content, PathBuf::from("/tmp/site/content"));
        assert!(paths.index_db.ends_with("index.db"));
        assert!(paths.assets.ends_with("images"));
        assert!(paths.assets.starts_with(&paths.static_dir));
    }

    #[test]
    fn project_paths_drop_dot_segments_from_the_root_too() {
        let build = Build::default();
        let assets = Assets::default();
        let paths = ProjectPaths::new("/tmp/./site", &build, &assets);
        assert_eq!(paths.root, PathBuf::from("/tmp/site"));
        assert_eq!(paths.output, PathBuf::from("/tmp/site/dist"));
    }

    #[test]
    fn assets_defaults_derive_url_prefix_from_dir() {
        let assets = Assets::default();
        assert_eq!(assets.dir, "images");
        assert_eq!(assets.url_prefix(), "/images/");
        assert_eq!(assets.naming, AssetNaming::Sha256);
        assert!(assets.shard);
        assert!(assets.validate().is_empty());
    }

    #[test]
    fn assets_url_prefix_override_is_normalized() {
        let assets = Assets {
            dir: "media/img".into(),
            url_prefix: Some("/cdn/assets".into()),
            ..Assets::default()
        };
        assert_eq!(assets.url_prefix(), "/cdn/assets/");
        assert_eq!(assets.normalized_dir(), "media/img");
    }

    #[test]
    fn assets_reject_traversal_and_bad_hash_length() {
        let assets = Assets {
            dir: "../outside".into(),
            hash_length: 4,
            ..Assets::default()
        };
        let issues = assets.validate();
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(assets.effective_hash_length(), 8, "越界值应被夹紧");
    }

    #[test]
    fn nested_assets_dir_maps_to_nested_path_and_url() {
        let assets = Assets {
            dir: "media/2026".into(),
            ..Assets::default()
        };
        let paths = ProjectPaths::new("/tmp/site", &Build::default(), &assets);
        assert!(paths.assets.ends_with(Path::new("media").join("2026")));
        assert_eq!(assets.url_prefix(), "/media/2026/");
    }
}
