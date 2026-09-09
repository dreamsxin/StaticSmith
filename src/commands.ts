/**
 * 菜单命令表：菜单栏与命令面板的唯一来源。
 *
 * Word / WPS 的菜单栏是「命令的家」——想做什么就去菜单里找，找不到才怀疑它不存在；
 * 而选项卡（我们的标签页）只回答「现在看什么」。这里就按这条分工组织：
 * 菜单 = 动词（保存、生成、发布、加粗、插链接），标签 = 名词（内容、外观、体检）。
 *
 * 两个界面共用一份表是刻意的：命令面板与菜单栏各写一遍，迟早出现「面板里有、
 * 菜单里没有」的分裂，而那正是用户抱怨「不知道能干什么」的来源。
 */
import { open } from '@tauri-apps/plugin-dialog'
import { reactive } from 'vue'

import { isProject } from './api'
import { actions, isDirty, isTemplateDirty, store } from './store'
import { applyMode, goTo, modes, saveLayout, ui } from './ui'

export interface Command {
  id: string
  label: string
  /** 右侧灰字：快捷键或补充说明 */
  hint?: string
  /** 不可用时置灰而不是隐藏：藏起来会让人以为功能不存在 */
  disabled?: boolean
  /** 勾选态（视图开关这类） */
  checked?: boolean
  /** 不可逆或影响线上的动作，菜单里标红 */
  danger?: boolean
  run: () => void | Promise<void>
}

export type MenuEntry = Command | { separator: true }

export interface Menu {
  label: string
  items: MenuEntry[]
}

export function isSeparator(entry: MenuEntry): entry is { separator: true } {
  return 'separator' in entry
}

/**
 * 右键菜单（上下文菜单）。
 *
 * 与菜单栏共用 `MenuEntry` 这一套形状：项目怎么画、禁用怎么表现、危险项怎么标红
 * 只有一份实现。设计文档原先写「不做右键」，理由是「两处入口不一致的维护成本」——
 * 共用形状 + 由调用方现场组装条目，正是消掉那份成本的办法，所以改成做。
 *
 * 只放**针对某个对象**的动作（这一篇、这个栏目）。全局动作留在菜单栏：
 * 右键是看不见的入口，把只在这里出现的功能藏进去等于没做。
 */
export const contextMenu = reactive({
  open: false,
  x: 0,
  y: 0,
  items: [] as MenuEntry[],
})

/**
 * 打开右键菜单的那个元素。
 *
 * 记下来是为了关闭后把焦点还回去：右键菜单常常从「⋯」按钮打开，
 * 用 Esc 取消之后焦点不能落在已经被移除的浮层上（那等于回到 body，
 * 下一次 Tab 要从页面开头重走一遍）。
 */
let contextOpener: HTMLElement | null = null

export function openContextMenu(event: MouseEvent, items: MenuEntry[]) {
  // 阻止 WebView 的原生菜单：那份菜单里只有「重新加载」这类对用户无意义的项
  event.preventDefault()
  event.stopPropagation()
  const opener = event.currentTarget
  contextOpener = opener instanceof HTMLElement ? opener : null
  contextMenu.items = items
  contextMenu.x = event.clientX
  contextMenu.y = event.clientY
  contextMenu.open = true
}

/** `restore`：Esc 这类「我不做了」把焦点还给打开它的元素；点了命令则由那条命令决定去哪。 */
export function closeContextMenu(options: { restore?: boolean } = {}) {
  contextMenu.open = false
  contextMenu.items = []
  if (options.restore) contextOpener?.focus()
  contextOpener = null
}

/** 选目录。新建 / 打开站点都要它。 */
async function pickDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false })
  return typeof selected === 'string' ? selected : null
}

async function openSite() {
  const path = await pickDirectory()
  if (!path) return
  if (!(await isProject(path))) {
    actions.notify('error', '该目录下没有 staticsmith.toml；要新建请用「新建站点…」')
    return
  }
  await actions.openProject(path)
}

/**
 * 新建站点。
 *
 * 起始页有标题输入框，菜单里没有——所以用目录名当站点标题，之后在「设置 → 站点信息」
 * 改。为一个字段弹一层对话框不值得，而目录名通常就是站点名。
 */
async function createSite() {
  const path = await pickDirectory()
  if (!path) return
  const name = path.split(/[\\/]/).filter(Boolean).pop() ?? '我的静态站'
  await actions.initProject(path, name)
}

/** 编辑器是否可用：没打开文章时，选区类命令一律置灰。 */
function noEditor(): boolean {
  return ui.editor === null
}

/**
 * 「保存」作用于**当前正在编辑的东西**。
 *
 * 外观页看着一个模板时是模板，其余情况是文章——这是 Word/WPS 那条预期：
 * Ctrl+S 永远保存眼前这份，不需要先想清楚「我按的是哪个保存」。
 *
 * 菜单项、Ctrl+S、编辑器工具条按钮都从这里取，三处各写一遍必然出现
 * 「按钮灰着但快捷键能存」这类分裂。
 */
export function saveTarget(): { label: string; disabled: boolean; run: () => Promise<void> } {
  if (ui.tab === 'layouts' && store.currentTemplate) {
    return {
      label: '保存当前模板',
      disabled: !isTemplateDirty.value || store.busy,
      run: () => actions.saveTemplate(),
    }
  }
  return {
    label: '保存当前文章',
    disabled: !isDirty.value || store.busy,
    run: () => actions.saveContent(),
  }
}

/** Ctrl+S 的落点。置灰时什么也不做，但按键仍被吞掉（不能让 WebView 弹「保存网页」）。 */
export async function saveCurrent() {
  const target = saveTarget()
  if (target.disabled) return
  await target.run()
}

/**
 * 定位到内容搜索框。
 *
 * 先把人送到能搜的地方再聚焦：以前 Ctrl+F 的监听挂在列表栏组件里，
 * 停在「体检」页或把列表栏收起来时按下去毫无反应——一个快捷键在一半界面里失效，
 * 用户学到的是「这个键不好用」，不是「这个键有前提」。
 */
export function focusSearch() {
  openList()
  ui.requestFocusSearch = true
}

/**
 * 把人送到内容页并确保列表栏开着。
 *
 * 侧栏里的表单（搜索、新建、替换）都要先满足这个前置条件。写成一处：
 * 三个入口各写一遍，迟早有一个忘了处理「列表栏被收起来」，那一项就成了死按钮。
 */
function openList() {
  goTo('content')
  if (!ui.showList) {
    // 手动打开就跟着关掉自动收放，否则下一次窗口变化又把它收回去
    ui.listOverride = true
    ui.showList = true
  }
}

/**
 * 重新读取磁盘并报一声。
 *
 * 通知只能加在这一层：`actions.refresh()` 被保存、批量、导入、生成等多条路径复用，
 * 加进去的话每次保存都要弹一条「已重新读取」。
 */
async function refreshFromDisk() {
  await actions.refresh()
  actions.notify('success', '已重新读取磁盘上的内容与模板')
}

/**
 * 当前可用的菜单。
 *
 * 每次读取都重算：禁用态、勾选态、最近站点列表都依赖当前状态，缓存只会让菜单
 * 显示上一次的样子。
 */
export function menus(): Menu[] {
  const recent = store.recent.slice(0, 5)
  const save = saveTarget()
  return [
    {
      label: '站点',
      items: [
        { id: 'site.new', label: '新建站点…', run: createSite },
        { id: 'site.open', label: '打开站点…', run: openSite },
        ...(recent.length > 0 ? [{ separator: true } as MenuEntry] : []),
        ...recent.map((item) => ({
          id: `site.recent:${item.path}`,
          label: item.title || item.path,
          hint: item.path,
          run: () => actions.openProject(item.path),
        })),
        { separator: true },
        {
          id: 'content.save',
          label: save.label,
          hint: 'Ctrl+S',
          disabled: save.disabled,
          run: saveCurrent,
        },
        { separator: true },
        {
          id: 'build.incremental',
          label: '生成（只重算受影响的页面）',
          hint: 'Ctrl+Enter',
          disabled: store.busy,
          run: () => actions.build('incremental'),
        },
        {
          id: 'build.full',
          label: '完整重建',
          hint: 'Ctrl+Shift+Enter',
          disabled: store.busy,
          run: () => actions.build('full'),
        },
        { id: 'build.reveal', label: '在文件管理器中打开产物', run: () => actions.revealOutput() },
        { separator: true },
        { id: 'go.deploy', label: '发布…', run: () => goTo('deploy') },
        {
          id: 'site.import',
          label: '从别的站点导入内容…',
          hint: 'Hugo / Jekyll',
          run: () => goTo('settings'),
        },
        { separator: true },
        { id: 'site.close', label: '关闭站点', run: () => actions.closeProject() },
      ],
    },
    {
      label: '编辑',
      items: [
        {
          id: 'edit.find',
          label: '查找内容',
          hint: 'Ctrl+Shift+F（编辑器里 Ctrl+F 是找这一篇内部）',
          run: focusSearch,
        },
        {
          id: 'edit.find-in-file',
          label: '在这一篇里查找 / 替换…',
          hint: 'Ctrl+F',
          disabled: noEditor(),
          run: () => ui.editor?.find(),
        },
        {
          id: 'edit.replace',
          label: '跨文件替换…',
          hint: '只改正文，front matter 不动；先干跑再落盘',
          run: () => {
            openList()
            ui.requestReplace = true
          },
        },
        {
          id: 'edit.new',
          label: '新建文章…',
          run: () => {
            goTo('content')
            ui.requestNewContent = true
          },
        },
        {
          id: 'edit.section',
          label: '新建栏目…',
          hint: 'content/ 下的一层目录',
          run: () => {
            goTo('content')
            ui.requestNewSection = true
          },
        },
        { separator: true },
        {
          id: 'edit.bold',
          label: '加粗',
          hint: 'Ctrl+B',
          disabled: noEditor(),
          run: () => ui.editor?.bold(),
        },
        {
          id: 'edit.italic',
          label: '斜体',
          hint: 'Ctrl+I',
          disabled: noEditor(),
          run: () => ui.editor?.italic(),
        },
        {
          id: 'edit.code',
          label: '行内代码',
          disabled: noEditor(),
          run: () => ui.editor?.code(),
        },
        { separator: true },
        {
          id: 'edit.heading',
          label: '标题（二级）',
          disabled: noEditor(),
          run: () => ui.editor?.heading(),
        },
        {
          id: 'edit.quote',
          label: '引用',
          disabled: noEditor(),
          run: () => ui.editor?.quote(),
        },
        {
          id: 'edit.bullet',
          label: '无序列表',
          disabled: noEditor(),
          run: () => ui.editor?.bullet(),
        },
      ],
    },
    {
      label: '插入',
      items: [
        {
          id: 'insert.link',
          label: '链接',
          hint: 'Ctrl+K',
          disabled: noEditor(),
          run: () => ui.editor?.link(),
        },
        {
          id: 'insert.file',
          label: '图片或附件…',
          hint: '也可直接粘贴 / 拖入编辑器',
          disabled: noEditor(),
          run: () => ui.editor?.pickFile(),
        },
        {
          id: 'insert.assets',
          label: '媒体库',
          hint: '复用已上传的资源',
          disabled: noEditor(),
          run: () => ui.editor?.assets(),
        },
      ],
    },
    {
      label: '视图',
      items: [
        // 模式排在最前：它一次决定整套布局，单个窗格的开关是它之后的微调
        ...modes.map((spec) => ({
          id: `view.mode.${spec.id}`,
          label: `${spec.label}模式`,
          hint: spec.hint,
          checked: ui.mode === spec.id,
          run: () => applyMode(spec.id),
        })),
        { separator: true },
        {
          id: 'view.list',
          label: '内容列表栏',
          checked: ui.showList,
          run: () => {
            // 手动动过之后不再随窗口宽度自动收放
            ui.listOverride = true
            ui.showList = !ui.showList
          },
        },
        {
          id: 'view.preview',
          label: '预览栏',
          checked: ui.showPreview,
          run: () => {
            ui.previewOverride = true
            ui.showPreview = !ui.showPreview
          },
        },
        {
          id: 'view.props',
          label: '属性面板',
          hint: 'front matter 的表单；也可以直接在 +++ 里改',
          checked: ui.showProps,
          run: () => {
            ui.showProps = !ui.showProps
            saveLayout()
          },
        },
        {
          id: 'view.toolbar',
          label: '格式工具条',
          hint: '加粗、标题、插图这些按钮；快捷键不受影响',
          checked: ui.showToolbar,
          run: () => {
            ui.showToolbar = !ui.showToolbar
            saveLayout()
          },
        },
        { separator: true },
        {
          id: 'view.palette',
          label: '命令面板（也能跳到任意页面）',
          hint: 'Ctrl+P',
          run: () => {
            ui.paletteOpen = true
          },
        },
      ],
    },
    {
      label: '工具',
      items: [
        { id: 'audit.seo', label: 'SEO 体检', run: () => runAudit('seo') },
        { id: 'audit.links', label: '站内死链体检', run: () => runAudit('links') },
        { id: 'audit.media', label: '媒体资源体检', run: () => runAudit('media') },
        { separator: true },
        {
          id: 'preview.server',
          label: store.previewServer ? '停止本地预览服务器' : '启动本地预览服务器',
          hint: store.previewServer ?? '仅监听 127.0.0.1',
          run: () => actions.togglePreviewServer(),
        },
        {
          id: 'build.auto',
          label: '保存后自动生成',
          checked: store.autoBuild,
          run: () => actions.setAutoBuild(!store.autoBuild),
        },
        { separator: true },
        {
          id: 'project.refresh',
          label: '重新读取磁盘上的内容与模板',
          run: refreshFromDisk,
        },
      ],
    },
    {
      label: '帮助',
      items: [
        {
          id: 'help.docs',
          label: '项目文档（浏览器打开）',
          run: () => actions.openInBrowser('https://github.com/dreamsxin/StaticSmith#readme'),
        },
        {
          id: 'help.shortcuts',
          label: '快捷键与全部命令',
          hint: 'Ctrl+P',
          run: () => {
            ui.paletteOpen = true
          },
        },
      ],
    },
  ]
}

/** 三种体检都先切到「体检」页再跑：结果显示在那里，跑完却停在别处等于没反馈。 */
async function runAudit(kind: 'seo' | 'links' | 'media') {
  goTo('audit')
  // announce：人主动点的这次要报结果，自动重跑的那些保持安静
  if (kind === 'seo') await actions.auditSeo({ announce: true })
  else if (kind === 'links') await actions.auditLinks({ announce: true })
  else await actions.auditMedia({ announce: true })
}

/** 命令面板用的扁平列表：菜单里的每一项都能被搜到，分隔符与置灰项除外。 */
export function flatCommands(): Array<Command & { group: string }> {
  return menus().flatMap((menu) =>
    menu.items
      .filter((entry): entry is Command => !isSeparator(entry) && !entry.disabled)
      .map((command) => ({ ...command, group: menu.label })),
  )
}
