/**
 * 全局应用状态。
 *
 * 用 `reactive` 单例而不是 Pinia：状态只有一份（同时只能打开一个项目），
 * 引入额外状态库带来的收益小于心智负担。
 */
import { computed, reactive, readonly } from 'vue'
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'

import * as api from './api'
import type {
  AssetRecord,
  BuildMode,
  BuildPlan,
  BuildReport,
  DeployReport,
  OutputFile,
  PageSummary,
  ProjectSummary,
  RecentEntry,
  SiteConfig,
  TemplateInfo,
} from './api'

export type ToastKind = 'success' | 'error' | 'info'

export interface Toast {
  id: number
  kind: ToastKind
  message: string
}

interface State {
  project: ProjectSummary | null
  /** 最近打开的站点，起始页用 */
  recent: RecentEntry[]
  /** 当前编辑的内容源路径 */
  currentSource: string | null
  currentRaw: string
  /** 上次保存时的正文快照，用来判断是否有未保存改动 */
  savedRaw: string
  /** 有未保存改动时被拦下的待打开页面 */
  pendingPage: PageSummary | null
  /** 当前编辑的模板名 */
  currentTemplate: string | null
  currentTemplateSource: string
  previewHtml: string
  /** 本地预览服务器地址，未启动时为 null */
  previewServer: string | null
  /** 正在预览的产物地址（标签页、分页页等非内容页），null 表示预览当前文章 */
  previewTarget: string | null
  /** 产物清单，生成后刷新 */
  outputs: OutputFile[]
  /** 已登记的媒体资源，编辑器复用时用 */
  assets: AssetRecord[]
  plan: BuildPlan | null
  lastBuild: BuildReport | null
  lastDeploy: DeployReport | null
  progress: string
  busy: boolean
  error: string | null
  toasts: Toast[]
  /** 磁盘上被外部编辑器改动、界面尚未刷新的提示 */
  externalChange: boolean
}

const state = reactive<State>({
  project: null,
  recent: [],
  currentSource: null,
  currentRaw: '',
  savedRaw: '',
  pendingPage: null,
  currentTemplate: null,
  currentTemplateSource: '',
  previewHtml: '',
  previewServer: null,
  previewTarget: null,
  outputs: [],
  assets: [],
  plan: null,
  lastBuild: null,
  lastDeploy: null,
  progress: '',
  busy: false,
  error: null,
  toasts: [],
  externalChange: false,
})

/** 编辑器里有未保存改动。切换文章、关闭项目前据此拦一道。 */
export const isDirty = computed(
  () => state.currentSource !== null && state.currentRaw !== state.savedRaw,
)

let toastId = 0

/** 弹一条通知。错误停留更久，因为用户往往需要读完整句。 */
function notify(kind: ToastKind, message: string) {
  const id = ++toastId
  state.toasts.push({ id, kind, message })
  const ttl = kind === 'error' ? 8000 : 3500
  setTimeout(() => {
    state.toasts = state.toasts.filter((t) => t.id !== id)
  }, ttl)
}

/**
 * 统一的错误处理与 busy 标记，避免每个组件各写一遍 try/catch。
 *
 * 失败一律弹通知：之前只写进 `state.error`，而调用点常常不显示它，
 * 结果就是「点了没反应」——比报错更难排查。
 */
async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
  state.busy = true
  state.error = null
  try {
    return await action()
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    state.error = message
    notify('error', message)
    return undefined
  } finally {
    state.busy = false
  }
}

export const store = readonly(state)

export const actions = {
  notify,

  dismissToast(id: number) {
    state.toasts = state.toasts.filter((t) => t.id !== id)
  },

  /** 读取最近打开的站点。已被删除或移动的条目由后端自动清理。 */
  async loadRecent() {
    const items = await run(() => api.recentProjects())
    if (items) state.recent = items
  },

  async forgetRecent(path: string) {
    const items = await run(() => api.forgetProject(path))
    if (items) state.recent = items
  },

  async openProject(path: string) {
    const summary = await run(() => api.openProject(path))
    if (summary) {
      state.project = summary
      state.externalChange = false
      await this.recomputePlan()
      await this.loadOutputs()
      notify('success', `已打开 ${summary.config.site.title}`)
    }
  },

  /** 刷新产物清单。标签页、分页页这些非内容页只在这里能看到。 */
  async loadOutputs() {
    const files = await run(() => api.listOutputs())
    if (files) state.outputs = files
  },

  /**
   * 预览某个产物（标签页、分页页、sitemap…）。
   *
   * 这些页面不是内容文件，没法走内存预览，只能由本地服务器提供；
   * 因此未启动时先自动起一个，不用用户先去点「启动本地服务器」。
   */
  async previewOutput(url: string) {
    if (!state.previewServer) {
      const base = await run(() => api.startPreviewServer())
      if (!base) return
      state.previewServer = base
    }
    state.previewTarget = url
  },

  /** 回到「预览当前文章」。 */
  clearPreviewTarget() {
    state.previewTarget = null
  },

  async initProject(path: string, title: string) {
    const created = await run(() => api.initProject(path, title))
    if (created) await this.openProject(path)
  },

  async refresh() {
    if (!state.project) return
    const summary = await run(() => api.projectSummary())
    if (summary) {
      state.project = summary
      state.externalChange = false
    }
  },

  closeProject() {
    void api.closeProject()
    state.project = null
    state.currentSource = null
    state.currentRaw = ''
    state.savedRaw = ''
    state.pendingPage = null
    state.previewHtml = ''
    // 预览服务器随会话在 Rust 侧一起停止，这里只清界面状态。
    state.previewServer = null
    state.previewTarget = null
    state.outputs = []
    state.assets = []
    state.plan = null
    // 回到起始页时刷新最近列表，刚关闭的站点应排在最前。
    void this.loadRecent()
  },

  /** 请求打开一篇内容。有未保存改动时先问，不直接丢弃。 */
  async requestOpenContent(page: PageSummary) {
    if (page.source === state.currentSource) return
    if (isDirty.value) {
      state.pendingPage = page
      return
    }
    await this.openContent(page)
  },

  /** 处理「未保存改动」的三种选择。 */
  async resolvePending(choice: 'save' | 'discard' | 'cancel') {
    const target = state.pendingPage
    state.pendingPage = null
    if (!target || choice === 'cancel') return
    if (choice === 'save') {
      await this.saveContent()
      if (isDirty.value) return // 保存失败就留在原处
    }
    await this.openContent(target)
  },

  async openContent(page: PageSummary) {
    const raw = await run(() => api.readContent(page.source))
    if (raw !== undefined) {
      state.currentSource = page.source
      state.currentRaw = raw
      state.savedRaw = raw
      state.currentTemplate = null
      // 打开文章即回到文章预览，否则预览还停在上次点开的标签页上。
      state.previewTarget = null
      await this.refreshPreview()
    }
  },

  /** 新建内容并立即打开编辑。 */
  async createContent(title: string, section: string) {
    const source = await run(() => api.createContent({ title, section }))
    if (!source) return
    await this.refresh()
    notify('success', `已创建 ${source}（草稿）`)
    const page = state.project?.pages.find((p) => p.source === source)
    if (page) await this.openContent(page as PageSummary)
  },

  /**
   * 删除一篇内容。
   *
   * 只删源文件；已生成的 HTML 留在产物目录里，由下一次构建按 `orphaned_pages` 清理，
   * 所以这里直接把返回的计划写进状态，界面能看到「将清理 N 个产物」。
   */
  async deleteContent(page: PageSummary) {
    const plan = await run(() => api.deleteContent(page.source))
    if (!plan) return
    state.plan = plan
    if (state.currentSource === page.source) {
      state.currentSource = null
      state.currentRaw = ''
      state.savedRaw = ''
      state.previewHtml = ''
    }
    if (state.pendingPage?.source === page.source) state.pendingPage = null
    await this.refresh()
    notify('success', `已删除 ${page.source}，生成时会清理它的产物`)
  },

  /** 读取已登记的媒体资源，编辑器「媒体库」用。 */
  async loadAssets() {
    const items = await run(() => api.listAssets())
    if (items) state.assets = items
  },

  /** 凭证是否已存在系统凭据管理器里。查询失败按「没有」处理。 */
  async hasSecret(account: string): Promise<boolean> {
    try {
      return await api.hasSecret(account)
    } catch {
      return false
    }
  },

  async deleteSecret(account: string) {
    const done = await run(() => api.deleteSecret(account))
    if (done !== undefined) notify('success', '凭证已从系统凭据管理器移除')
  },


  /**
   * 切换本地预览服务器。
   *
   * 内存预览取不到图片与 CSS（iframe srcdoc 没有文件访问权限），
   * 开启服务器后预览走 HTTP，与线上完全一致。
   */
  async togglePreviewServer() {
    if (state.previewServer) {
      await run(() => api.stopPreviewServer())
      state.previewServer = null
      notify('info', '本地预览服务器已停止')
      return
    }
    const url = await run(() => api.startPreviewServer())
    if (url) {
      state.previewServer = url
      notify('success', `本地预览服务器：${url}`)
    }
  },

  /** 在系统默认浏览器里打开地址。 */
  async openInBrowser(url: string) {
    await run(() => openUrl(url))
  },

  /** 在文件管理器中定位输出目录。 */
  async revealOutput() {
    const dir = await run(() => api.outputDir())
    if (dir) await run(() => revealItemInDir(dir))
  },

  setRaw(raw: string) {
    state.currentRaw = raw
  },

  async saveContent() {
    if (!state.currentSource) return
    const raw = state.currentRaw
    const plan = await run(() => api.saveContent(state.currentSource!, raw))
    if (plan) {
      state.savedRaw = raw
      state.plan = plan
      await this.refresh()
      await this.refreshPreview()
      notify('success', `已保存，待生成 ${plan.pages.length} 个页面`)
    }
  },

  async refreshPreview() {
    if (!state.currentSource) return
    const html = await run(() => api.previewPage(state.currentSource!))
    if (html !== undefined) state.previewHtml = html
  },

  /**
   * 保存粘贴或拖入的文件，返回站内地址。
   *
   * 只负责落盘与登记；把地址插到正文哪个位置由编辑器组件决定（它才知道光标在哪）。
   */
  async saveAsset(file: File): Promise<string | undefined> {
    const bytes = new Uint8Array(await file.arrayBuffer())
    const asset = await run(() => api.saveAsset(file.name || 'pasted', bytes))
    if (!asset) return undefined
    // 新资源要出现在媒体库里，否则刚粘的图片在「复用」列表里找不到。
    void this.loadAssets()
    notify(
      'success',
      asset.deduplicated
        ? `已复用相同内容的资源 ${asset.file_name}`
        : `已保存资源 ${asset.file_name}`,
    )
    return asset.url
  },

  async openTemplate(template: TemplateInfo) {
    const source = await run(() => api.readTemplate(template.name))
    if (source !== undefined) {
      state.currentTemplate = template.name
      state.currentTemplateSource = source
    }
  },

  setTemplateSource(source: string) {
    state.currentTemplateSource = source
  },

  /** 保存全局组件后拿到级联影响范围，界面据此提示「影响 N 个页面」。 */
  async saveTemplate() {
    if (!state.currentTemplate) return
    const plan = await run(() =>
      api.saveTemplate(state.currentTemplate!, state.currentTemplateSource),
    )
    if (plan) {
      state.plan = plan
      await this.refresh()
      notify('success', `${state.currentTemplate} 已保存，影响 ${plan.pages.length} 个页面`)
    }
  },

  async recomputePlan(mode: BuildMode = 'incremental') {
    const plan = await run(() => api.buildPlan(mode))
    if (plan) state.plan = plan
  },

  async build(mode: BuildMode) {
    const report = await run(() => api.runBuild(mode))
    if (report) {
      state.lastBuild = report
      await this.recomputePlan()
      await this.refresh()
      await this.loadOutputs()
      notify(
        'success',
        `生成完成：${report.pages_rendered} 个页面 / ${report.files_written} 个文件，${report.duration_ms} ms`,
      )
      for (const warning of report.warnings) notify('info', warning)
    }
  },

  async saveConfig(config: SiteConfig) {
    const issues = await run(() => api.saveConfig(config))
    if (issues) {
      await this.refresh()
      notify('success', '设置已保存')
    }
  },

  async saveSecret(account: string, secret: string) {
    const done = await run(() => api.saveSecret(account, secret))
    if (done !== undefined) notify('success', '凭证已写入系统凭据管理器')
  },

  async checkDeploy() {
    const done = await run(() => api.checkDeploy())
    if (done !== undefined) notify('success', '连接与凭证检查通过')
  },

  async deploy() {
    const report = await run(() => api.deploySite())
    if (report) {
      state.lastDeploy = report
      notify('success', `发布完成：上传 ${report.uploaded.length} 个文件`)
    }
  },

  dismissError() {
    state.error = null
  },
}

// 后端事件 → 界面状态。监听器在应用生命周期内常驻，无需注销。
void api.onBuildProgress((p) => {
  state.progress = p.total > 0 ? `${p.phase} ${p.current}/${p.total}` : p.phase
})

void api.onDeployProgress((p) => {
  state.progress = `${p.message}（${p.current}/${p.total}）`
})

void api.onProjectChanged(() => {
  // 外部编辑器改了模板或内容：提示用户刷新，而不是自动覆盖正在编辑的内容。
  state.externalChange = true
})
