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
    page_size: number
    minify: boolean
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

// ---------------------------------------------------------------- 项目

export const initProject = (path: string, title?: string) =>
  invoke<string[]>('init_project', { path, title })

export const isProject = (path: string) => invoke<boolean>('is_project', { path })

export const openProject = (path: string) => invoke<ProjectSummary>('open_project', { path })

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
