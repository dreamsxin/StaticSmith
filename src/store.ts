/**
 * 全局应用状态。
 *
 * 用 `reactive` 单例而不是 Pinia：状态只有一份（同时只能打开一个项目），
 * 引入额外状态库带来的收益小于心智负担。
 */
import { reactive, readonly } from 'vue'

import * as api from './api'
import type {
  BuildMode,
  BuildPlan,
  BuildReport,
  DeployReport,
  PageSummary,
  ProjectSummary,
  SiteConfig,
  TemplateInfo,
} from './api'

interface State {
  project: ProjectSummary | null
  /** 当前编辑的内容源路径 */
  currentSource: string | null
  currentRaw: string
  /** 当前编辑的模板名 */
  currentTemplate: string | null
  currentTemplateSource: string
  previewHtml: string
  /** 本地预览服务器地址，未启动时为 null */
  previewServer: string | null
  plan: BuildPlan | null
  lastBuild: BuildReport | null
  lastDeploy: DeployReport | null
  progress: string
  busy: boolean
  error: string | null
  /** 磁盘上被外部编辑器改动、界面尚未刷新的提示 */
  externalChange: boolean
}

const state = reactive<State>({
  project: null,
  currentSource: null,
  currentRaw: '',
  currentTemplate: null,
  currentTemplateSource: '',
  previewHtml: '',
  previewServer: null,
  plan: null,
  lastBuild: null,
  lastDeploy: null,
  progress: '',
  busy: false,
  error: null,
  externalChange: false,
})

/** 统一的错误处理与 busy 标记，避免每个组件各写一遍 try/catch。 */
async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
  state.busy = true
  state.error = null
  try {
    return await action()
  } catch (err) {
    state.error = err instanceof Error ? err.message : String(err)
    return undefined
  } finally {
    state.busy = false
  }
}

export const store = readonly(state)

export const actions = {
  async openProject(path: string) {
    const summary = await run(() => api.openProject(path))
    if (summary) {
      state.project = summary
      state.externalChange = false
      await this.recomputePlan()
    }
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
    state.previewHtml = ''
    // 预览服务器随会话在 Rust 侧一起停止，这里只清界面状态。
    state.previewServer = null
    state.plan = null
  },

  async openContent(page: PageSummary) {
    const raw = await run(() => api.readContent(page.source))
    if (raw !== undefined) {
      state.currentSource = page.source
      state.currentRaw = raw
      state.currentTemplate = null
      await this.refreshPreview()
    }
  },

  /** 新建内容并立即打开编辑。 */
  async createContent(title: string, section: string) {
    const source = await run(() => api.createContent({ title, section }))
    if (!source) return
    await this.refresh()
    const page = state.project?.pages.find((p) => p.source === source)
    if (page) await this.openContent(page as PageSummary)
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
      return
    }
    const url = await run(() => api.startPreviewServer())
    if (url) state.previewServer = url
  },

  setRaw(raw: string) {
    state.currentRaw = raw
  },

  async saveContent() {
    if (!state.currentSource) return
    const plan = await run(() => api.saveContent(state.currentSource!, state.currentRaw))
    if (plan) {
      state.plan = plan
      await this.refresh()
      await this.refreshPreview()
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
    state.progress = asset.deduplicated
      ? `已复用相同内容的资源 ${asset.file_name}`
      : `已保存资源 ${asset.file_name}`
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
    }
  },

  async saveConfig(config: SiteConfig) {
    const issues = await run(() => api.saveConfig(config))
    if (issues) await this.refresh()
  },

  async saveSecret(account: string, secret: string) {
    await run(() => api.saveSecret(account, secret))
  },

  async checkDeploy() {
    await run(() => api.checkDeploy())
  },

  async deploy() {
    const report = await run(() => api.deploySite())
    if (report) state.lastDeploy = report
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
