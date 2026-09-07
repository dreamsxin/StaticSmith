/**
 * Rust 侧 IPC 命令的类型化封装。
 *
 * 字段命名与 Rust 结构体一致（serde 默认保留 snake_case），
 * 因此这里的接口就是后端契约的唯一声明处，改后端记得同步这里。
 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type BuildMode = 'full' | 'incremental'
export type TemplateKind = 'layout' | 'component' | 'page' | 'partial'
export type DeployKind = 'none' | 'git' | 'ftp'
export type AssetNaming = 'sha256' | 'md5' | 'original'

export interface SiteConfig {
  site: {
    title: string
    description: string
    base_url: string
    language: string
    extra: Record<string, unknown>
  }
  build: {
    output_dir: string
    content_dir: string
    theme_dir: string
    template_dir: string
    static_dir: string
    page_size: number
    minify: boolean
    generate_sitemap: boolean
    generate_feed: boolean
    feed_limit: number
    /** false 表示定时发布：未来日期的文章暂不进产物 */
    publish_future: boolean
  }
  assets: {
    /** 相对 static_dir 的子目录 */
    dir: string
    naming: AssetNaming
    hash_length: number
    shard: boolean
    max_size_mb: number
    url_prefix?: string | null
  }
  taxonomy: {
    enabled: boolean
    /** front matter 字段名，如 tags / categories */
    name: string
    /** URL 前缀，如 tags → /tags/ */
    slug: string
    title: string
    list_template: string
    term_template: string
  }
  /**
   * 多分类维度。写了它就以它为准，`taxonomy` 退化为旧写法。
   *
   * 界面目前只编辑单数的 `taxonomy`，但保存时会把这个数组原样带回去，
   * 不会把手写的 `[[taxonomies]]` 抹掉。
   */
  taxonomies: Array<{
    enabled: boolean
    name: string
    slug: string
    title: string
    list_template: string
    term_template: string
  }>

  deploy: {
    type: DeployKind
    git?: {
      remote: string
      branch: string
      commit_message: string
      auth_type: string
      ssh_key_path?: string | null
    } | null
    ftp?: {
      host: string
      port: number
      username: string
      password_env?: string | null
      remote_path: string
      sftp: boolean
    } | null
  }
}

export interface PageSummary {
  source: string
  title: string
  url: string
  template: string
  section: string
  is_index: boolean
  draft: boolean
  date: string | null
  /** 非草稿但这次不会进产物：日期还没到，且站点开了定时发布 */
  scheduled: boolean
  tags: string[]
}


export interface TemplateInfo {
  name: string
  kind: TemplateKind
  path: string
  hash: string
  dependencies: string[]
}

export interface TemplateNode {
  name: string
  cyclic: boolean
  children: TemplateNode[]
}

export interface BuildRecord {
  id: number
  finished_at: string
  mode: string
  pages_written: number
  duration_ms: number
}

export interface ProjectSummary {
  root: string
  config: SiteConfig
  pages: PageSummary[]
  layouts: TemplateInfo[]
  components: TemplateInfo[]
  templates: TemplateInfo[]
  recent_builds: BuildRecord[]
  dirty_pages: string[]
}

export interface BuildPlan {
  mode: BuildMode
  pages: string[]
  changed_templates: string[]
  affected_templates: string[]
  total_pages: number
  orphaned_pages: string[]
}

export interface BuildReport {
  mode: BuildMode
  pages_rendered: number
  files_written: number
  assets_copied: number
  removed_files: string[]
  duration_ms: number
  warnings: string[]
}

export interface DeployReport {
  target: string
  uploaded: string[]
  deleted: string[]
  skipped: number
  duration_ms: number
  commit: string | null
  warnings: string[]
}

export interface BuildProgress {
  phase: string
  current: number
  total: number
}

export interface DeployProgress {
  message: string
  current: number
  total: number
}

export interface ChangeSet {
  templates: string[]
  content: string[]
  other: string[]
}

export interface SavedAsset {
  content_hash: string
  file_name: string
  /** 相对 static_dir 的路径 */
  relative_path: string
  /** 可直接写进 Markdown 的站内地址 */
  url: string
  size: number
  /** 命中已有文件，未实际写盘 */
  deduplicated: boolean
}

export interface AssetRecord {
  content_hash: string
  path: string
  url: string
  size: number
  created_at: string
}

// ---------------------------------------------------------------- 项目

export const initProject = (path: string, title?: string) =>
  invoke<string[]>('init_project', { path, title })

export const isProject = (path: string) => invoke<boolean>('is_project', { path })

export const openProject = (path: string) => invoke<ProjectSummary>('open_project', { path })

/** 最近打开的站点，最新在前。 */
export interface RecentEntry {
  path: string
  title: string
  opened_at: string
}

export const recentProjects = () => invoke<RecentEntry[]>('recent_projects')

export const forgetProject = (path: string) =>
  invoke<RecentEntry[]>('forget_project', { path })

export const closeProject = () => invoke<void>('close_project')

export const projectSummary = () => invoke<ProjectSummary>('project_summary')

// ---------------------------------------------------------------- 配置

export const readConfig = () => invoke<SiteConfig>('read_config')

export const saveConfig = (config: SiteConfig) => invoke<string[]>('save_config', { config })

// ---------------------------------------------------------------- 内容

export const listPages = () => invoke<PageSummary[]>('list_pages')

export const readContent = (source: string) => invoke<string>('read_content', { source })

export const saveContent = (source: string, raw: string) =>
  invoke<BuildPlan>('save_content', { args: { source, raw } })

export const deleteContent = (source: string) => invoke<BuildPlan>('delete_content', { source })

export const previewPage = (source: string) => invoke<string>('preview_page', { source })

/** front matter 字段，与 Rust 的 `FrontMatter` 对应。 */
export interface FrontMatter {
  title: string
  date: string | null
  template: string | null
  slug: string | null
  description: string
  tags: string[]
  /** SEO 关键词，未写时页面会回退到 tags */
  keywords: string[]
  /** 旧地址，构建会为每个生成重定向页 */
  aliases: string[]
  draft: boolean
  weight: number
  extra: Record<string, unknown>
}

/**
 * 属性面板的改动。省略的字段保持不动，空串与空数组表示删掉这个键。
 */
export interface FrontMatterPatch {
  title?: string
  description?: string
  date?: string
  template?: string
  slug?: string
  tags?: string[]
  keywords?: string[]
  aliases?: string[]
  draft?: boolean
  weight?: number
}

/** 读 front matter：传的是编辑器缓冲区里的文本，未保存也能读。 */
export const readFrontMatter = (raw: string) => invoke<FrontMatter>('read_front_matter', { raw })

/** 把属性改动折算成新的源文，正文与其他键原样保留。 */
export const applyFrontMatter = (raw: string, patch: FrontMatterPatch) =>
  invoke<string>('apply_front_matter', { raw, patch })


/** 新建内容的请求体，字段名与 Rust 的 `NewContent` 一致。 */
export interface NewContentRequest {
  section: string
  title: string
  slug?: string | null
  template?: string | null
  description?: string
  tags?: string[]
  /** 缺省为 true：新建内容默认是草稿 */
  draft?: boolean
}

export const createContent = (request: NewContentRequest) =>
  invoke<string>('create_content', { request })

// ---------------------------------------------------------------- 本地预览服务器

export const startPreviewServer = (port?: number) =>
  invoke<string>('start_preview_server', { port })

export const stopPreviewServer = () => invoke<void>('stop_preview_server')

export const previewServerUrl = () => invoke<string | null>('preview_server_url')

// ---------------------------------------------------------------- 媒体资源

/**
 * 保存粘贴或拖入的文件。
 *
 * 走 base64 而不是字节数组：JSON IPC 传 `number[]` 会把每个字节膨胀成 2-4 个字符，
 * base64 只有 4/3 的开销。
 */
export async function saveAsset(fileName: string, bytes: Uint8Array): Promise<SavedAsset> {
  return invoke<SavedAsset>('save_asset', {
    fileName,
    dataBase64: toBase64(bytes),
  })
}

export const listAssets = () => invoke<AssetRecord[]>('list_assets')

/** 产物类型，与 Rust 的 `OutputKind` 对应。 */
export type OutputKind = 'page' | 'pagination' | 'taxonomy' | 'sitemap' | 'feed' | 'asset'

export interface OutputFile {
  /** 相对输出目录的路径 */
  path: string
  /** 站内地址，可直接拼到预览服务器后面 */
  url: string
  kind: OutputKind
  size: number
}

/** 产物清单。还没生成过时返回空数组。 */
export const listOutputs = () => invoke<OutputFile[]>('list_outputs')

// ---------------------------------------------------------------- SEO 体检

export type SeoSeverity = 'error' | 'warn' | 'hint'

export interface SeoIssue {
  /** 相对 content/ 的源路径；站点级问题为空串 */
  source: string
  url: string
  title: string
  severity: SeoSeverity
  /** 稳定的规则标识，如 description.missing */
  code: string
  message: string
}

export interface SeoReport {
  checked: number
  errors: number
  warnings: number
  hints: number
  /** 0-100 的粗略健康度，只用于趋势对比 */
  score: number
  issues: SeoIssue[]
}

/** 体检当前内存里的页面，不依赖产物，保存后立刻可用。 */
export const auditSeo = () => invoke<SeoReport>('audit_seo')

// ---------------------------------------------------------------- 媒体资源体检

export interface MediaFile {
  /** 相对 static_dir 的路径 */
  path: string
  url: string
  size: number
}

export interface MissingRef {
  url: string
  /** 引用它的文件，相对站点根 */
  referenced_by: string[]
}

export interface MediaReport {
  total: number
  total_size: number
  /** 没有任何内容、模板或主题引用的文件 */
  unused: MediaFile[]
  /** 删掉 unused 能回收的字节数 */
  reclaimable: number
  /** 引用了却不存在的地址 */
  missing: MissingRef[]
}

export interface MediaRemoved {
  removed: string[]
  freed: number
}

export const auditMedia = () => invoke<MediaReport>('audit_media')

// ---------------------------------------------------------------- 站内链接体检

export interface BrokenLink {
  /** 页面里原样写着的地址，便于在源文件里搜到出处 */
  href: string
  /** 解析后实际找不到的站内地址 */
  url: string
  /** 引用它的页面地址 */
  referenced_by: string[]
}

export interface LinkReport {
  /** 产物目录是否存在。false 表示还没生成过，其余字段都是 0 */
  built: boolean
  pages: number
  internal: number
  /** 站外链接只计数，不发网络请求 */
  external: number
  broken: BrokenLink[]
}

/** 体检产物里的站内链接，需要先生成一次。 */
export const auditLinks = () => invoke<LinkReport>('audit_links')

// ---------------------------------------------------------------- 批量动作

/** 跳过的一篇及原因。批量动作逐篇独立，跳过要说清为什么。 */
export interface BatchSkipped {
  source: string
  reason: string
}

export interface BatchMoved {
  from: string
  to: string
  /** 是否补了旧地址 */
  alias_added: boolean
}

export interface BatchReport {
  changed: string[]
  skipped: BatchSkipped[]
  plan: BuildPlan
}

export interface BatchMoveReport {
  moved: BatchMoved[]
  skipped: BatchSkipped[]
  plan: BuildPlan
}

/** 批量增删标签。加什么、去什么分开传，避免把各篇原有标签洗掉。 */
export const batchEditTags = (sources: string[], add: string[], remove: string[]) =>
  invoke<BatchReport>('batch_edit_tags', { args: { sources, add, remove } })

/** 批量发布（draft=false）或收回草稿（draft=true）。 */
export const batchSetDraft = (sources: string[], draft: boolean) =>
  invoke<BatchReport>('batch_set_draft', { sources, draft })

/** 批量搬到另一个栏目，`keepAliases` 为真时补旧地址。 */
export const batchMove = (sources: string[], toSection: string, keepAliases: boolean) =>
  invoke<BatchMoveReport>('batch_move', {
    args: { sources, to_section: toSection, keep_aliases: keepAliases },
  })

/** 批量删除内容。不可逆。 */
export const batchDelete = (sources: string[]) =>
  invoke<BatchReport>('batch_delete', { sources })

// ---------------------------------------------------------------- 栏目


export interface Section {
  /** 相对 content/ 的目录，根目录是空串 */
  path: string
  /** 有索引页就是它的标题，否则是目录名 */
  title: string
  url: string
  /** 栏目索引页的源文件；为 null 表示这个栏目打不开列表页 */
  index_source: string | null
  /** 直属文章数（不含索引页与子栏目） */
  pages: number
  drafts: number
  /** 直接子栏目的路径 */
  children: string[]
}

export interface SectionCreated {
  path: string
  index_source: string
}

export interface SectionRenamed {
  from: string
  to: string
  moved: number
  /** 补了旧地址的文章数 */
  aliases_added: number
}

export const listSections = () => invoke<Section[]>('list_sections')

export const createSection = (path: string, title: string) =>
  invoke<SectionCreated>('create_section', { path, title })

/** 栏目改名。`keepAliases` 为真时给每篇文章补旧地址，老链接靠重定向页继续可用。 */
export const renameSection = (from: string, to: string, keepAliases: boolean) =>
  invoke<SectionRenamed>('rename_section', { args: { from, to, keep_aliases: keepAliases } })

/** 删除空栏目，返回删除后的栏目清单。里面还有文章时后端报错，不会连带删除。 */
export const removeSection = (path: string) => invoke<Section[]>('remove_section', { path })


/** 删除媒体文件，不可撤销。只允许删资源目录内的文件。 */
export const removeMedia = (paths: string[]) => invoke<MediaRemoved>('remove_media', { paths })



/** 分块转换，避免大文件时 `String.fromCharCode(...)` 参数过多导致栈溢出。 */
function toBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000
  let binary = ''
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK))
  }
  return btoa(binary)
}

// ---------------------------------------------------------------- 模板

export const listTemplates = () => invoke<TemplateInfo[]>('list_templates')

export const templateTree = (root: string) => invoke<TemplateNode>('template_tree', { root })

export const readTemplate = (name: string) => invoke<string>('read_template', { name })

export const saveTemplate = (name: string, source: string) =>
  invoke<BuildPlan>('save_template', { name, source })

// ---------------------------------------------------------------- 构建与发布

export const buildPlan = (mode: BuildMode) => invoke<BuildPlan>('build_plan', { mode })

export const runBuild = (mode: BuildMode) => invoke<BuildReport>('run_build', { mode })

export const outputDir = () => invoke<string>('output_dir')

export const deploySite = () => invoke<DeployReport>('deploy_site')

export const checkDeploy = () => invoke<void>('check_deploy')

export const saveSecret = (account: string, secret: string) =>
  invoke<void>('save_secret', { account, secret })

export const hasSecret = (account: string) => invoke<boolean>('has_secret', { account })

export const deleteSecret = (account: string) => invoke<void>('delete_secret', { account })

// ---------------------------------------------------------------- 事件

export const onBuildProgress = (handler: (p: BuildProgress) => void): Promise<UnlistenFn> =>
  listen<BuildProgress>('build://progress', (e) => handler(e.payload))

export const onDeployProgress = (handler: (p: DeployProgress) => void): Promise<UnlistenFn> =>
  listen<DeployProgress>('deploy://progress', (e) => handler(e.payload))

export const onProjectChanged = (handler: (c: ChangeSet) => void): Promise<UnlistenFn> =>
  listen<ChangeSet>('project://changed', (e) => handler(e.payload))
