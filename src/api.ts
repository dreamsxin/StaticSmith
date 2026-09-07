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
    /** URL 前缀，如 tags → /tags/ */
    slug: string
    title: string
    list_template: string
    term_template: string
  }
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
