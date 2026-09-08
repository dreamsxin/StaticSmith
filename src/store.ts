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
  FrontMatter,
  FrontMatterPatch,
  LinkReport,
  McpStatus,
  MediaReport,
  OutputFile,
  PageSummary,
  ProjectSummary,
  RecentEntry,
  Section,
  SeoReport,
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
  /** 当前源文的 front matter 字段，属性面板用 */
  frontMatter: FrontMatter | null
  /** 有未保存改动时被拦下的待打开页面 */
  pendingPage: PageSummary | null
  /** 当前编辑的模板名 */
  currentTemplate: string | null
  currentTemplateSource: string
  /** 打开或上次保存时的模板源码快照，用来判断模板是否有未保存改动 */
  savedTemplateSource: string
  previewHtml: string
  /** 本地预览服务器地址，未启动时为 null */
  previewServer: string | null
  /** MCP 服务端状态（AI 接入），未启动时为 null。权限由服务端回报，前端不自己记 */
  mcp: McpStatus | null
  /** 正在预览的产物地址（标签页、分页页等非内容页），null 表示预览当前文章 */
  previewTarget: string | null
  /** 产物清单，生成后刷新 */
  outputs: OutputFile[]
  /** 已登记的媒体资源，编辑器复用时用 */
  assets: AssetRecord[]
  /** SEO 体检结论，保存或生成后刷新 */
  seo: SeoReport | null
  /** 栏目清单（含根目录），打开项目与增删改栏目后刷新 */
  sections: Section[]
  /** 媒体资源体检结论，按需刷新（要扫盘，不跟着每次保存跑） */
  media: MediaReport | null
  /** 站内链接体检结论，按需刷新（读产物，需要先生成过） */
  links: LinkReport | null
  plan: BuildPlan | null
  lastBuild: BuildReport | null
  lastDeploy: DeployReport | null
  progress: string
  busy: boolean
  /** 保存后自动增量生成，让服务器预览与产物跟着变 */
  autoBuild: boolean
  /**
   * 最近一次失败的消息。
   *
   * 失败一律弹通知（瞬时出口），这个字段只作为**留在界面上**的那一份，
   * 目前唯一的渲染点是起始页的 `alert`（打不开站点时那句话必须留着，
   * 通知飘走了用户就不知道为什么进不去）。主界面里需要用户处置的错误应就地显示在
   * 出问题的那一块（设置页字段旁、发布面板内），不再做全局常驻错误条。
   */
  error: string | null
  toasts: Toast[]
  /** 磁盘上被外部编辑器改动、界面尚未刷新的提示 */
  externalChange: boolean
}

const AUTO_BUILD_KEY = 'staticsmith.autoBuild'

const state = reactive<State>({
  project: null,
  recent: [],
  currentSource: null,
  currentRaw: '',
  savedRaw: '',
  frontMatter: null,
  pendingPage: null,
  currentTemplate: null,
  currentTemplateSource: '',
  savedTemplateSource: '',
  previewHtml: '',
  previewServer: null,
  mcp: null,
  previewTarget: null,
  outputs: [],
  assets: [],
  seo: null,
  sections: [],
  media: null,
  links: null,
  plan: null,
  lastBuild: null,
  lastDeploy: null,
  progress: '',
  busy: false,
  autoBuild: localStorage.getItem(AUTO_BUILD_KEY) === '1',
  error: null,
  toasts: [],
  externalChange: false,
})

/** 编辑器里有未保存改动。切换文章、关闭项目前据此拦一道。 */
export const isDirty = computed(
  () => state.currentSource !== null && state.currentRaw !== state.savedRaw,
)

/**
 * 模板源码有未保存改动。
 *
 * 与文章分开一个判断，因为两者可以同时处于「打开」状态：外观页改模板时，
 * 内容页可能还有一篇没保存的文章。保存命令按当前在看哪一个来分派。
 */
export const isTemplateDirty = computed(
  () => state.currentTemplate !== null && state.currentTemplateSource !== state.savedTemplateSource,
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
 *
 * 忙态用**计数器**而不是布尔：`build`、`saveContent` 内部会嵌套调用好几次 `run`，
 * 布尔标志在一次点击里 true/false 跳变数次——按钮闪烁，`disabled` 在整段操作
 * 结束前就解除，用户能在中途二次点击。归零时才算真的做完，顺手把进度文字清空：
 * 停在 `render 42/42` 上比不显示进度更糟，它看起来像「还在跑」。
 */
let inflight = 0

async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
  inflight += 1
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
    inflight -= 1
    if (inflight === 0) {
      state.busy = false
      state.progress = ''
    }
  }
}

/**
 * 把一整段复合动作圈成**一次**忙态。
 *
 * 「保存文章」实际是保存 → 重读摘要 → 重算 front matter → 刷预览 → 跑 SEO 体检五次
 * 调用，它们是**顺序**而非嵌套的：只靠 `run` 里的计数器，每一步之间计数都会归零，
 * 按钮照样闪。所以复合动作在入口处先占一格，中途任何一步归零都不算完。
 *
 * 新增复合动作时照这个套：只要函数体里出现两次以上 `run`/`await this.xxx`，就该包起来。
 */
async function busySpan<T>(body: () => Promise<T>): Promise<T> {
  inflight += 1
  state.busy = true
  try {
    return await body()
  } finally {
    inflight -= 1
    if (inflight === 0) {
      state.busy = false
      state.progress = ''
    }
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
    await busySpan(async () => {
      const summary = await run(() => api.openProject(path))
      if (!summary) return
      state.project = summary
      state.externalChange = false
      await this.recomputePlan()
      await this.loadOutputs()
      await this.loadSections()
      await this.auditSeo()
      notify('success', `已打开 ${summary.config.site.title}`)
    })
  },

  /** 刷新产物清单。标签页、分页页这些非内容页只在这里能看到。 */
  async loadOutputs() {
    const files = await run(() => api.listOutputs())
    if (files) state.outputs = files
  },

  /** 刷新栏目清单。栏目就是 content/ 下的目录，这里额外带上「有没有索引页」。 */
  async loadSections() {
    const sections = await run(() => api.listSections())
    if (sections) state.sections = sections
  },

  /**
   * 扫描待导入目录，只读。
   *
   * 迁移一次影响几十上百篇，所以界面也走「先看清再落盘」：这里返回逐篇的
   * 「会写到哪、front matter 变成什么样、哪里需要人看一下」。
   */
  async scanImport(dir: string, section: string) {
    return await run(() => api.scanImport(dir, section))
  },

  /** 导入内容。目标已存在的跳过，不覆盖。 */
  async importContent(dir: string, section: string) {
    const report = await run(() => api.importContent(dir, section))
    if (!report) return undefined
    const warned = report.warnings.length
    notify(
      warned > 0 ? 'info' : 'success',
      warned > 0
        ? `导入 ${report.imported.length} 篇，${warned} 处需要人看一下`
        : `导入 ${report.imported.length} 篇`,
    )
    await this.refresh()
    await this.loadSections()
    await this.recomputePlan()
    return report
  },

  /** 打包当前外观。不含 content/ 与 static/，所以给别人也不会带走内容。 */
  async exportTheme(archive: string, manifest: api.ThemeManifest) {
    const report = await run(() => api.exportTheme(archive, manifest))
    if (!report) return undefined
    notify(
      'success',
      `已打包「${manifest.name}」：模板 ${report.templates} 个、资源 ${report.assets} 个`,
    )
    return report
  },

  /** 只读：主题包会写哪些文件、哪些会被覆盖。 */
  async scanTheme(archive: string) {
    return await run(() => api.scanTheme(archive))
  },

  /**
   * 装主题包。
   *
   * 换外观等于所有页面的模板都变了，所以装完刷新组件树与增量计划——
   * 「待生成 N / M」会直接跳成全站，这正是应该让人看到的事实。
   */
  async importTheme(archive: string, overwrite: boolean) {
    const report = await run(() => api.importTheme(archive, overwrite))
    if (!report) return undefined
    notify(
      'success',
      `已装「${report.manifest.name}」：写入 ${report.written.length} 个文件，跳过 ${report.skipped.length} 个`,
    )
    await this.refresh()
    await this.recomputePlan()
    return report
  },


  /** 新建栏目：建目录并写一张索引页，否则栏目列表页打不开。 */
  async createSection(path: string, title: string) {
    const created = await run(() => api.createSection(path, title))
    if (!created) return
    notify('success', `已新建栏目 ${created.path}`)
    await this.refresh()
    await this.loadSections()
    await this.recomputePlan()
  },

  // -------------------------------------------------------------- 批量动作

  /**
   * 批量动作的收尾：刷新页面清单与计划，把跳过的原因摊给用户。
   *
   * 没有撤销栈：批量改的是磁盘上的源文，做一份可靠的反向操作等于自己实现一个
   * 小型版本控制，而这个项目里内容本来就该在 Git 下。所以宁可把「改了几篇、
   * 跳过几篇、为什么」说清楚，让用户自己决定下一步。
   */
  async afterBatch(changed: number, skipped: api.BatchSkipped[], plan: BuildPlan) {
    await busySpan(async () => {
      state.plan = plan
      await this.refresh()
      await this.loadSections()
      if (state.seo) await this.auditSeo()
      if (state.autoBuild && changed > 0) await this.build('incremental', { quiet: true })

      if (skipped.length === 0) {
        notify('success', `已处理 ${changed} 篇`)
        return
      }
      // 只报第一条原因：十几条堆在提示里没人看，剩下的数量给出来就够了
      const first = `${skipped[0].source}：${skipped[0].reason}`
      const rest = skipped.length > 1 ? `，另有 ${skipped.length - 1} 篇被跳过` : ''
      notify(changed > 0 ? 'info' : 'error', `已处理 ${changed} 篇；跳过 ${first}${rest}`)
    })
  },

  /** 批量增删标签。 */
  async batchEditTags(sources: string[], add: string[], remove: string[]) {
    const report = await run(() => api.batchEditTags(sources, add, remove))
    if (!report) return
    await this.afterBatch(report.changed.length, report.skipped, report.plan)
  },

  /**
   * 干跑一个批量动作，返回每篇会发生什么。
   *
   * 只给搬动与删除用：它们不可逆或会改地址，值得先看一眼；加标签、切草稿
   * 反手就能改回来，多一次确认只是白点。
   */
  async batchPreview(sources: string[], action: api.BatchAction) {
    return await run(() => api.batchPreview(sources, action))
  },


  /** 批量发布或收回草稿。 */
  async batchSetDraft(sources: string[], draft: boolean) {
    const report = await run(() => api.batchSetDraft(sources, draft))
    if (!report) return
    await this.afterBatch(report.changed.length, report.skipped, report.plan)
  },

  /** 批量搬到另一个栏目。默认补旧地址，老链接经重定向页继续可用。 */
  async batchMove(sources: string[], toSection: string, keepAliases = true) {
    const report = await run(() => api.batchMove(sources, toSection, keepAliases))
    if (!report) return
    // 当前打开的文章可能刚被搬走，源路径已经失效
    if (report.moved.some((m) => m.from === state.currentSource)) {
      state.currentSource = null
      state.currentRaw = ''
      state.savedRaw = ''
      state.previewHtml = ''
    }
    await this.afterBatch(report.moved.length, report.skipped, report.plan)
  },

  /** 批量删除。不可逆，调用方必须已经二次确认。 */
  async batchDelete(sources: string[]) {
    const report = await run(() => api.batchDelete(sources))
    if (!report) return
    if (state.currentSource && report.changed.includes(state.currentSource)) {
      state.currentSource = null
      state.currentRaw = ''
      state.savedRaw = ''
      state.previewHtml = ''
    }
    await this.afterBatch(report.changed.length, report.skipped, report.plan)
  },


  /**
   * 栏目改名。
   *
   * `keepAliases` 默认开着：整理结构不该顺手打断所有外部链接。它会给每篇文章
   * 补上旧地址，构建后旧地址是一张重定向页。
   */
  async renameSection(from: string, to: string, keepAliases = true) {
    const report = await run(() => api.renameSection(from, to, keepAliases))
    if (!report) return
    notify(
      'success',
      keepAliases
        ? `已把 ${report.from} 改名为 ${report.to}，${report.aliases_added} 篇补了旧地址`
        : `已把 ${report.from} 改名为 ${report.to}（未保留旧地址）`,
    )
    // 当前打开的文章可能刚被搬走，源路径已经失效
    if (state.currentSource?.startsWith(`${report.from}/`)) {
      state.currentSource = null
      state.currentRaw = ''
      state.savedRaw = ''
      state.previewHtml = ''
    }
    await this.refresh()
    await this.loadSections()
    await this.recomputePlan()
  },

  /** 删除空栏目。里面还有文章时后端会报错，不会连带删除。 */
  async removeSection(path: string) {
    const sections = await run(() => api.removeSection(path))
    if (!sections) return
    state.sections = sections
    notify('success', `已删除栏目 ${path}`)
    await this.refresh()
    await this.recomputePlan()
  },

  /**
   * 改栏目元信息（标题、简介、排序权重）。
   *
   * 元信息写在索引页的 front matter 上，缺索引页的栏目会顺手补一张——
   * 栏目就是目录，它的介绍就该在那张列表页里，不额外发明一个栏目配置文件。
   */
  async saveSectionMeta(path: string, meta: api.SectionMeta) {
    const sections = await run(() => api.saveSectionMeta(path, meta))
    if (!sections) return
    state.sections = sections
    notify('success', `已更新栏目「${meta.title}」`)
    await this.refresh()
    await this.recomputePlan()
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
    state.frontMatter = null
    state.pendingPage = null
    state.currentTemplate = null
    state.currentTemplateSource = ''
    state.savedTemplateSource = ''
    state.previewHtml = ''
    // 预览服务器随会话在 Rust 侧一起停止，这里只清界面状态。
    state.previewServer = null
    // MCP 服务端同理：它挂在 Session 上，关站点就停了
    state.mcp = null
    state.previewTarget = null
    state.outputs = []
    state.assets = []
    state.seo = null
    state.media = null
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
      await this.loadFrontMatter()
      await this.refreshPreview()
    }
  },

  /**
   * 读出当前缓冲区的 front matter，属性面板据此回填。
   *
   * 读的是编辑器里的文本而不是磁盘：用户可能刚在源文里手改了标题还没保存，
   * 表单必须跟着那份文本，否则一改属性就会把手改的内容覆盖回去。
   */
  async loadFrontMatter() {
    if (state.currentSource === null) {
      state.frontMatter = null
      return
    }
    try {
      state.frontMatter = await api.readFrontMatter(state.currentRaw)
    } catch {
      // front matter 暂时写坏了（正在手改）不该弹错，面板自己会提示不可用
      state.frontMatter = null
    }
  },

  /**
   * 用表单改动折算出新的源文。
   *
   * 折算在 Rust 侧做：TOML 的转义与保序只该有一份实现，
   * 而且正文、注释、未知键都得原样保留。结果写回编辑器缓冲区，仍需用户保存。
   */
  async patchFrontMatter(patch: FrontMatterPatch) {
    if (state.currentSource === null) return
    const raw = await run(() => api.applyFrontMatter(state.currentRaw, patch))
    if (raw === undefined) return
    state.currentRaw = raw
    await this.loadFrontMatter()
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
      state.frontMatter = null
      state.previewHtml = ''
    }
    if (state.pendingPage?.source === page.source) state.pendingPage = null
    await this.refresh()
    notify('success', `已删除 ${page.source}，生成时会清理它的产物`)
    if (state.autoBuild) await this.build('incremental', { quiet: true })
  },

  /** 读取已登记的媒体资源，编辑器「媒体库」用。 */
  async loadAssets() {
    const items = await run(() => api.listAssets())
    if (items) state.assets = items
  },

  /**
   * SEO 体检。
   *
   * 读的是内存里的页面，不看产物，所以保存后立刻反映；
   * 与 MCP 的 `audit_seo` 同源，界面结论和 AI Agent 拿到的完全一致。
   */
  async auditSeo(options: { announce?: boolean } = {}) {
    const report = await run(() => api.auditSeo())
    if (!report) return
    state.seo = report
    // 只有「人主动点」的那次才报结果：保存、构建、批量动作都会顺带重跑体检，
    // 在那些路径上弹通知就是噪音（违 6.8 的另一半）。
    if (!options.announce) return
    notify(
      'success',
      report.issues.length > 0
        ? `SEO 体检：${report.issues.length} 处待处理`
        : 'SEO 体检：没有发现问题',
    )
  },

  /**
   * 媒体资源体检。
   *
   * 要遍历资源目录与全部内容/模板文本，比 SEO 体检重，所以只在打开面板或
   * 手动刷新时跑，不挂在每次保存上。
   */
  async auditMedia(options: { announce?: boolean } = {}) {
    const report = await run(() => api.auditMedia())
    if (!report) return
    state.media = report
    if (!options.announce) return
    notify(
      'success',
      report.unused.length + report.missing.length > 0
        ? `媒体体检：${report.unused.length} 个没人用、${report.missing.length} 处地址不存在`
        : `媒体体检：${report.total} 个文件都在用`,
    )
  },

  /**
   * 站内链接体检。
   *
   * 读产物而不是源文件——只有产物才知道分页页、标签页与 slug 覆盖后的真实
   * 地址。所以没生成过时报告里 `built = false`，面板据此提示「先生成一次」。
   */
  async auditLinks(options: { announce?: boolean } = {}) {
    const report = await run(() => api.auditLinks())
    if (!report) return
    state.links = report
    if (!options.announce) return
    if (!report.built) {
      notify('error', '死链体检：还没生成过产物，先生成一次')
      return
    }
    notify(
      'success',
      report.broken.length > 0
        ? `死链体检：${report.broken.length} 条站内链接指向不存在的页面`
        : `死链体检：${report.internal} 条站内链接都能打开`,
    )
  },


  /** 删除未引用的媒体文件。不可撤销，调用前必须已在界面里确认过。 */
  async removeMedia(paths: string[]) {
    if (!paths.length) return
    const result = await run(() => api.removeMedia(paths))
    if (!result) return
    notify(
      'success',
      `已删除 ${result.removed.length} 个文件，回收 ${(result.freed / 1024).toFixed(1)} KB`,
    )
    await this.auditMedia()
    await this.loadAssets()
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

  // -------------------------------------------------------------- AI 接入（MCP）

  /**
   * 读回 MCP 服务端的当前状态。
   *
   * 打开面板时调一次：服务端跟着 Session 活，切站点、重启应用都会停掉，
   * 前端自己记状态就会显示一个已经不存在的端点。
   */
  async loadMcpStatus() {
    const status = await run(() => api.mcpServerStatus())
    if (status !== undefined) state.mcp = status
  },

  /**
   * 启动 MCP 服务端。权限是启动参数，改权限等于重启（Rust 侧会先停再起）。
   *
   * 通知里带上权限：这条命令的后果是「谁能改我的站点」，不能只说「已启动」。
   */
  async startMcp(allowWrite: boolean, allowDeploy: boolean) {
    const status = await run(() => api.startMcpServer(allowWrite, allowDeploy))
    if (!status) return
    state.mcp = status
    const scope = status.allowDeploy
      ? '可读写并发布'
      : status.allowWrite
        ? '可读写内容与模板'
        : '只读'
    notify('success', `AI 接入已开启（${scope}）：${status.endpoint}`)
  },

  async stopMcp() {
    await run(() => api.stopMcpServer())
    state.mcp = null
    notify('info', 'AI 接入已关闭')
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
    await busySpan(async () => {
      const raw = state.currentRaw
      const plan = await run(() => api.saveContent(state.currentSource!, raw))
      if (!plan) return
      state.savedRaw = raw
      state.plan = plan
      await this.refresh()
      await this.loadFrontMatter()
      await this.refreshPreview()
      await this.auditSeo()
      notify('success', `已保存，待生成 ${plan.pages.length} 个页面`)
      if (state.autoBuild) await this.build('incremental', { quiet: true })
    })
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
      state.savedTemplateSource = source
    }
  },

  setTemplateSource(source: string) {
    state.currentTemplateSource = source
  },

  /** 保存全局组件后拿到级联影响范围，界面据此提示「影响 N 个页面」。 */
  async saveTemplate() {
    if (!state.currentTemplate) return
    await busySpan(async () => {
      const source = state.currentTemplateSource
      const plan = await run(() => api.saveTemplate(state.currentTemplate!, source))
      if (!plan) return
      state.savedTemplateSource = source
      state.plan = plan
      await this.refresh()
      notify('success', `${state.currentTemplate} 已保存，影响 ${plan.pages.length} 个页面`)
      if (state.autoBuild) await this.build('incremental', { quiet: true })
    })
  },

  async recomputePlan(mode: BuildMode = 'incremental') {
    const plan = await run(() => api.buildPlan(mode))
    if (plan) state.plan = plan
  },

  async build(mode: BuildMode, options: { quiet?: boolean } = {}) {
    await busySpan(async () => {
      const report = await run(() => api.runBuild(mode))
      if (!report) return
      state.lastBuild = report
      await this.recomputePlan()
      await this.refresh()
      await this.loadOutputs()
      // 链接体检读产物，生成一次结论就过期了；只在用户已经看过时才重跑，避免白扫盘。
      if (state.links) await this.auditLinks()
      // 服务器预览不用在这里做什么：预览服务器注入的脚本会发现版本号变了并自己刷新。

      if (!options.quiet) {
        notify(
          'success',
          `生成完成：${report.pages_rendered} 个页面 / ${report.files_written} 个文件，${report.duration_ms} ms`,
        )
      }
      // 警告合并成一条：逐条弹的话，一次生成能把通知区刷满一屏（违 6.8）
      if (report.warnings.length === 1) notify('info', report.warnings[0])
      else if (report.warnings.length > 1) {
        notify('info', `${report.warnings.length} 条生成警告：${report.warnings[0]} 等`)
      }
    })
  },

  /**
   * 保存后是否自动增量生成。
   *
   * 内存预览一直是即时的，但服务器预览与产物目录要等手动「生成」，
   * 于是「改完看不到」成了常态。开启后保存即重建，和现代前端的 dev server 一致；
   * 默认关闭，因为大站点的一次增量也要秒级，不该替用户决定。
   */
  setAutoBuild(on: boolean) {
    state.autoBuild = on
    localStorage.setItem(AUTO_BUILD_KEY, on ? '1' : '0')
    notify('info', on ? '保存后将自动增量生成' : '已关闭保存后自动生成')
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
