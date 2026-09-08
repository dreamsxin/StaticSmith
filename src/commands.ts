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

import { isProject } from './api'
import { actions, isDirty, store } from './store'
import { goTo, ui, type Tab } from './ui'

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
 * 当前可用的菜单。
 *
 * 每次读取都重算：禁用态、勾选态、最近站点列表都依赖当前状态，缓存只会让菜单
 * 显示上一次的样子。
 */
export function menus(): Menu[] {
  const recent = store.recent.slice(0, 5)
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
          label: '保存当前文章',
          hint: 'Ctrl+S',
          disabled: !isDirty.value,
          run: () => actions.saveContent(),
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
          hint: 'Ctrl+F',
          run: () => {
            goTo('content')
            ui.requestFocusSearch = true
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
        { separator: true },
        {
          id: 'insert.section',
          label: '栏目（在内容侧栏的分组头上新建）',
          run: () => goTo('content'),
        },
        { id: 'insert.import', label: '从别的站点导入内容…', run: () => goTo('settings') },
      ],
    },
    {
      label: '视图',
      items: [
        {
          id: 'view.list',
          label: '内容列表栏',
          checked: ui.showList,
          run: () => {
            ui.showList = !ui.showList
          },
        },
        {
          id: 'view.preview',
          label: '预览栏',
          checked: ui.showPreview,
          run: () => {
            ui.showPreview = !ui.showPreview
          },
        },
        { separator: true },
        {
          id: 'view.palette',
          label: '命令面板',
          hint: 'Ctrl+P',
          run: () => {
            ui.paletteOpen = true
          },
        },
        { separator: true },
        ...tabCommands(),
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
          run: () => actions.refresh(),
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

/** 「视图」里的跳页项：与标签页同一份定义，标签上写不下的用途放在 hint 里。 */
function tabCommands(): Command[] {
  const labels: Array<{ id: Tab; label: string }> = [
    { id: 'content', label: '内容' },
    { id: 'calendar', label: '日历（发布节奏）' },
    { id: 'layouts', label: '外观（模板与主题包）' },
    { id: 'audit', label: '体检（SEO / 死链 / 媒体）' },
    { id: 'build', label: '生成' },
    { id: 'deploy', label: '发布' },
    { id: 'settings', label: '设置' },
  ]
  return labels.map((item) => ({
    id: `go.${item.id}`,
    label: `切换到 ${item.label}`,
    checked: ui.tab === item.id,
    run: () => goTo(item.id),
  }))
}

/** 三种体检都先切到「体检」页再跑：结果显示在那里，跑完却停在别处等于没反馈。 */
async function runAudit(kind: 'seo' | 'links' | 'media') {
  goTo('audit')
  if (kind === 'seo') await actions.auditSeo()
  else if (kind === 'links') await actions.auditLinks()
  else await actions.auditMedia()
}

/** 命令面板用的扁平列表：菜单里的每一项都能被搜到，分隔符与置灰项除外。 */
export function flatCommands(): Array<Command & { group: string }> {
  return menus().flatMap((menu) =>
    menu.items
      .filter((entry): entry is Command => !isSeparator(entry) && !entry.disabled)
      .map((command) => ({ ...command, group: menu.label })),
  )
}
