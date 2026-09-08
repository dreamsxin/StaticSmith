<script setup lang="ts">
/**
 * 站点设置：`staticsmith.toml` 的可视化表单，外加一次性的内容导入。
 *
 * 按段显示（站点 / 构建 / 媒体 / 分类 / 导航 / 发布 / 导入），一次只看一段——
 * 之前是一张长表单，找一项要滚半屏。主题包不在这里，它换的是模板与主题资源，
 * 归「外观」页。
 */
import { computed, reactive, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'

import type { ImportCandidate, SiteConfig } from '../api'
import { actions, store } from '../store'

/** 表单持有一份可变副本，保存时才写回磁盘。 */
const form = reactive<SiteConfig>(clone(store.project?.config))

// ---------------------------------------------------------------- 内容导入

/**
 * 导入向导。
 *
 * 与命令行 `staticsmith import` 同一套判断（`scan` / `import`），界面只负责
 * 「选目录 → 看清单 → 确认」。迁移一次影响几十上百篇，所以扫描与落盘分成两步，
 * 不做「点一下直接导」。
 */
const importDir = ref('')
const importSection = ref('posts')
const candidates = ref<ImportCandidate[] | null>(null)

const importable = computed(
  () => candidates.value?.filter((candidate) => candidate.importable).length ?? 0,
)

/** 栏目建议用自己的 datalist：内容页的同名列表不一定挂载着。 */
const knownSections = computed(() =>
  store.sections.map((section) => section.path).filter((path) => path !== ''),
)

async function pickImportDir() {
  const selected = await open({ directory: true, multiple: false })
  if (typeof selected === 'string') {
    importDir.value = selected
    candidates.value = null
  }
}

async function scanImport() {
  candidates.value =
    (await actions.scanImport(importDir.value.trim(), importSection.value.trim())) ?? null
}

async function runImport() {
  const report = await actions.importContent(importDir.value.trim(), importSection.value.trim())
  if (!report) return
  // 导完重扫一次：已经进来的会变成「已存在」，还剩什么一目了然
  await scanImport()
}

/**
 * 设置页分段显示。
 *
 * 之前是一张长表单从站点信息一路滚到分类与导航，找一项要滚半屏，也看不出
 * 哪些字段是一伙的。现在按「一个问题一段」切开，一次只显示一段。
 */
type Section = 'site' | 'build' | 'assets' | 'taxonomy' | 'menu' | 'deploy' | 'import'

const sections: Array<{ id: Section; label: string; hint: string }> = [
  { id: 'site', label: '站点信息', hint: '标题、描述、地址、语言——模板里的 site.* 就是这些' },
  { id: 'build', label: '构建', hint: '目录、分页、压缩、sitemap / 订阅、定时发布' },
  { id: 'assets', label: '媒体', hint: '编辑器插入的图片存哪、怎么命名、地址前缀' },
  { id: 'taxonomy', label: '分类维度', hint: '标签、分类……每个维度生成一套总览页与词条页' },
  { id: 'menu', label: '导航菜单', hint: '头部导航的顺序与地址，加栏目改这里就够了' },
  { id: 'deploy', label: '发布', hint: '发布方式与目标；密码只存变量名，不写进配置文件' },
  { id: 'import', label: '导入内容', hint: '一次性动作：把 Hugo / Jekyll 的内容搬进来' },
]

const section = ref<Section>('site')

const sectionHint = computed(
  () => sections.find((item) => item.id === section.value)?.hint ?? '',
)




/** 与 Rust 侧 `Assets::url_prefix` 同样的推导规则，让用户改目录时能立刻看到效果。 */
const assetUrlPrefix = computed(() => {
  const override = form.assets.url_prefix?.trim()
  if (override) return `${override.replace(/\/+$/, '')}/`
  const dir = form.assets.dir.replace(/^\/+|\/+$/g, '')
  return dir ? `/${dir}/` : '/'
})

watch(
  () => store.project?.config,
  (config) => {
    Object.assign(form, clone(config))
    normalizeTaxonomies()
    normalizeMenu()
  },
)

/** store 是深只读的，深拷贝一份给表单编辑（拷贝会把只读性去掉）。 */
function clone(config: unknown): SiteConfig {
  if (config) return JSON.parse(JSON.stringify(config)) as SiteConfig
  return {
    site: { title: '', description: '', base_url: '', language: 'zh-CN', extra: {} },
    build: {
      output_dir: './dist',
      content_dir: './content',
      theme_dir: './themes/default',
      template_dir: './templates',
      static_dir: './static',
      page_size: 10,
      minify: true,
      generate_sitemap: true,
      generate_feed: true,
      feed_limit: 20,
      publish_future: true,
    },
    assets: {
      dir: 'images',
      naming: 'sha256',
      hash_length: 16,
      shard: true,
      max_size_mb: 32,
      url_prefix: null,
    },
    taxonomy: {
      enabled: true,
      name: 'tags',
      slug: 'tags',
      title: '标签',
      list_template: 'pages/tags.html',
      term_template: 'pages/tag.html',
    },
    taxonomies: [],
    menu: [],
    deploy: { type: 'none', git: null, ftp: null },
  }
}

/**
 * 分类维度统一按列表编辑。
 *
 * 配置文件有两种写法（单数 `[taxonomy]` 与数组 `[[taxonomies]]`），界面只呈现一种：
 * 读取时把单数段折进列表，保存时只有一个维度就写回单数段——
 * 免得用户只是改了个标题，配置文件的形状就被换掉。
 */
function normalizeTaxonomies() {
  if (form.taxonomies.length) return
  if (form.taxonomy.enabled) form.taxonomies = [{ ...form.taxonomy }]
}

function addTaxonomy() {
  form.taxonomies.push({
    enabled: true,
    name: 'categories',
    slug: 'categories',
    title: '分类',
    list_template: 'pages/tags.html',
    term_template: 'pages/tag.html',
  })
}

function removeTaxonomy(index: number) {
  form.taxonomies.splice(index, 1)
}

/**
 * 导航菜单按顺序编辑。
 *
 * 界面不暴露 weight：让人填数字来排序是本末倒置的。读取时按 weight 排好，
 * 保存时按行序重写成 1、2、3……手改配置的人照样能用 weight，两边看到的顺序一致。
 */
function normalizeMenu() {
  form.menu.sort((a, b) => a.weight - b.weight)
}

function addMenuItem() {
  form.menu.push({ name: '', url: '/', weight: form.menu.length + 1, blank: false })
}

function removeMenuItem(index: number) {
  form.menu.splice(index, 1)
}

function moveMenuItem(index: number, delta: number) {
  const target = index + delta
  if (target < 0 || target >= form.menu.length) return
  const [row] = form.menu.splice(index, 1)
  form.menu.splice(target, 0, row)
}

// 首次挂载也要排一次：watch 只在配置对象变化时触发
normalizeMenu()

/**
 * 按维度数量决定写回哪种形状，然后保存。
 *
 * 不改 `form` 本身：保存可能被后端校验驳回（比如两个维度用了同一个 slug），
 * 那时界面上的行必须还在，否则用户刚填的东西看起来凭空少了一条。
 */
function save() {
  const rows = form.taxonomies.map((row) => ({ ...row, enabled: true }))
  const payload = clone(form)
  // 顺序即 weight：界面上的第几行就是第几项
  payload.menu = form.menu.map((item, index) => ({
    ...item,
    name: item.name.trim(),
    url: item.url.trim(),
    weight: index + 1,
  }))
  if (rows.length === 0) {
    payload.taxonomy = { ...form.taxonomy, enabled: false }
    payload.taxonomies = []
  } else if (rows.length === 1) {
    payload.taxonomy = { ...rows[0] }
    payload.taxonomies = []
  } else {
    payload.taxonomies = rows
  }
  void actions.saveConfig(payload)
}


/** 切换发布方式时补齐对应配置段，避免保存时被后端校验拒绝。 */
function onDeployKindChange() {


  if (form.deploy.type === 'git' && !form.deploy.git) {
    form.deploy.git = {
      remote: '',
      branch: 'main',
      commit_message: '站点更新于 {{ now() }}',
      auth_type: 'token',
      ssh_key_path: null,
    }
  }
  if (form.deploy.type === 'ftp' && !form.deploy.ftp) {
    form.deploy.ftp = {
      host: '',
      port: 21,
      username: '',
      password_env: 'FTP_PASSWORD',
      remote_path: '/public_html',
      sftp: false,
    }
  }
}
</script>

<template>
  <section class="settings">
    <nav class="settings__nav" aria-label="设置分段">
      <button
        v-for="item in sections"
        :key="item.id"
        type="button"
        class="settings__nav-item"
        :class="{ active: section === item.id }"
        :aria-current="section === item.id ? 'page' : undefined"
        :title="item.hint"
        @click="section = item.id"
      >
        {{ item.label }}
      </button>
    </nav>
    <p class="build__muted settings__nav-hint">{{ sectionHint }}</p>

    <div v-if="section === 'site'" class="settings__panel">
      <h3>站点信息</h3>
      <label>标题<input v-model="form.site.title" type="text" /></label>
      <label>描述<input v-model="form.site.description" type="text" /></label>
      <label>站点地址<input v-model="form.site.base_url" type="text" /></label>
      <label>语言<input v-model="form.site.language" type="text" /></label>
      <p class="build__muted">
        这四项在模板里是 <code>site.title</code> 等；站点地址还决定 sitemap 与订阅能不能生成。
      </p>
    </div>

    <div v-if="section === 'build'" class="settings__panel">
      <h3>构建</h3>
      <label>内容目录<input v-model="form.build.content_dir" type="text" /></label>
      <label>模板目录<input v-model="form.build.template_dir" type="text" /></label>
      <label>主题目录<input v-model="form.build.theme_dir" type="text" /></label>
      <label>静态资源目录<input v-model="form.build.static_dir" type="text" /></label>
      <label>输出目录<input v-model="form.build.output_dir" type="text" /></label>
      <label>每页条数<input v-model.number="form.build.page_size" type="number" min="1" /></label>
      <label class="settings__checkbox">
        <input v-model="form.build.minify" type="checkbox" />
        压缩输出 HTML
      </label>
      <label class="settings__checkbox">
        <input v-model="form.build.generate_sitemap" type="checkbox" />
        生成 sitemap.xml
      </label>
      <label class="settings__checkbox">
        <input v-model="form.build.generate_feed" type="checkbox" />
        生成 Atom 订阅（feed.xml）
      </label>
      <label>
        订阅条目上限（0 为不限）
        <input v-model.number="form.build.feed_limit" type="number" min="0" />
      </label>
      <label class="settings__checkbox">
        <input v-model="form.build.publish_future" type="checkbox" />
        立即发布未来日期的文章
      </label>
      <p class="build__muted">
        取消勾选就是定时发布：日期晚于构建时刻的文章先不进产物，等定时构建到点再上线。
      </p>
      <p v-if="!form.site.base_url.trim()" class="build__muted">
        站点地址为空时会跳过 sitemap 与订阅——它们需要绝对地址。
      </p>
    </div>

    <div v-if="section === 'assets'" class="settings__panel">
      <h3>媒体资源</h3>
      <label>
        存放子目录（相对静态资源目录）
        <input v-model="form.assets.dir" type="text" />
      </label>
      <label>
        文件命名
        <select v-model="form.assets.naming">
          <option value="sha256">SHA-256 内容哈希</option>
          <option value="md5">MD5 内容哈希</option>
          <option value="original">保留原文件名</option>
        </select>
      </label>
      <label>
        哈希长度
        <input v-model.number="form.assets.hash_length" type="number" min="8" max="64" />
      </label>
      <label>
        单文件上限（MB，0 为不限）
        <input v-model.number="form.assets.max_size_mb" type="number" min="0" />
      </label>
      <label class="settings__checkbox">
        <input v-model="form.assets.shard" type="checkbox" />
        用哈希前两位分片存放
      </label>
      <p class="build__muted">当前资源地址前缀：<code>{{ assetUrlPrefix }}</code></p>
    </div>

    <div v-if="section === 'taxonomy'" class="settings__panel">
      <h3>分类维度</h3>
      <p class="build__muted">
        每个维度读一个 front matter 字段，生成总览页与词条页（分页沿用「每页条数」）。
        标签、分类、专栏都是同一种东西，只是字段与 URL 不同。
      </p>

      <div v-for="(tax, index) in form.taxonomies" :key="index" class="settings__row">
        <label>字段名<input v-model="tax.name" type="text" placeholder="tags" /></label>
        <label>URL 前缀<input v-model="tax.slug" type="text" placeholder="tags" /></label>
        <label>总览页标题<input v-model="tax.title" type="text" placeholder="标签" /></label>
        <label>总览模板<input v-model="tax.list_template" type="text" /></label>
        <label>词条模板<input v-model="tax.term_template" type="text" /></label>
        <p class="build__muted">
          front matter 写 <code>{{ tax.name || tax.slug }} = ["…"]</code>，
          生成 <code>/{{ tax.slug }}/</code> 与 <code>/{{ tax.slug }}/&lt;词条&gt;/</code>
        </p>
        <button type="button" class="page-list__danger" @click="removeTaxonomy(index)">
          移除这个维度
        </button>
      </div>

      <p v-if="!form.taxonomies.length" class="build__muted">
        当前不生成任何分类页。文章里的 tags 只会显示为纯文本。
      </p>
      <div class="build__actions">
        <button type="button" @click="addTaxonomy">添加维度</button>
      </div>
    </div>

    <div v-if="section === 'menu'" class="settings__panel">
      <h3>导航菜单</h3>
      <p class="build__muted">
        头部导航按这里的顺序渲染，加栏目不用改模板。分类维度（在「分类维度」那段配）由模板
        自己列出，不必在这里重复登记。站内地址要以 <code>/</code> 开头。
      </p>

      <div v-for="(item, index) in form.menu" :key="index" class="settings__row">
        <div class="settings__menu-row">
          <input v-model="item.name" type="text" placeholder="名称" aria-label="菜单名称" />
          <input v-model="item.url" type="text" placeholder="/posts/" aria-label="菜单地址" />
        </div>
        <label class="settings__checkbox">
          <input v-model="item.blank" type="checkbox" />
          新窗口打开
        </label>
        <div class="settings__menu-row">
          <button type="button" :disabled="index === 0" @click="moveMenuItem(index, -1)">上移</button>
          <button
            type="button"
            :disabled="index === form.menu.length - 1"
            @click="moveMenuItem(index, 1)"
          >
            下移
          </button>
          <button type="button" class="page-list__danger" @click="removeMenuItem(index)">
            移除
          </button>
        </div>
      </div>

      <p v-if="!form.menu.length" class="build__muted">
        没有配置菜单，模板会用自己写死的那几个链接。
      </p>
      <div class="build__actions">
        <button type="button" @click="addMenuItem">添加菜单项</button>
      </div>
    </div>

    <div v-if="section === 'deploy'" class="settings__panel">
      <h3>发布</h3>
      <label>
        方式
        <select v-model="form.deploy.type" @change="onDeployKindChange">
          <option value="none">暂不配置</option>
          <option value="git">Git</option>
          <option value="ftp">FTP / SFTP</option>
        </select>
      </label>

      <template v-if="form.deploy.type === 'git' && form.deploy.git">
        <label>远端地址<input v-model="form.deploy.git.remote" type="text" /></label>
        <label>分支<input v-model="form.deploy.git.branch" type="text" /></label>
        <label>提交信息<input v-model="form.deploy.git.commit_message" type="text" /></label>
        <label>
          认证方式
          <select v-model="form.deploy.git.auth_type">
            <option value="token">Token</option>
            <option value="ssh">SSH 私钥</option>
          </select>
        </label>
      </template>

      <template v-if="form.deploy.type === 'ftp' && form.deploy.ftp">
        <label>主机<input v-model="form.deploy.ftp.host" type="text" /></label>
        <label>端口<input v-model.number="form.deploy.ftp.port" type="number" /></label>
        <label>用户名<input v-model="form.deploy.ftp.username" type="text" /></label>
        <label>远端路径<input v-model="form.deploy.ftp.remote_path" type="text" /></label>
        <label>密码环境变量名<input v-model="form.deploy.ftp.password_env" type="text" /></label>
        <label class="settings__checkbox">
          <input v-model="form.deploy.ftp.sftp" type="checkbox" />
          使用 SFTP
        </label>
      </template>

      <p class="build__muted">
        密码与 Token 不会写入配置文件，请在「发布」页保存到系统凭据管理器。
      </p>
    </div>

    <div v-if="section === 'import'" class="build__panel">
      <h3>从别的站点导入内容</h3>
      <p class="build__muted">
        递归找 <code>.md</code> / <code>.markdown</code>，把 Hugo / Jekyll 的 YAML front matter
        转成 TOML。<strong>正文一个字节都不动</strong>；转不了的字段会写成注释留在文件里，
        并列进下面的提示，不会悄悄丢。命令行同样可用：<code>staticsmith import</code>。
      </p>

      <div class="settings__import-row">
        <input
          v-model="importDir"
          type="text"
          placeholder="待导入的目录"
          aria-label="待导入的目录"
        />
        <button type="button" :disabled="store.busy" @click="pickImportDir">选择目录…</button>
      </div>
      <label>
        导入到栏目（留空为根目录）
        <input v-model="importSection" type="text" list="import-known-sections" />
      </label>
      <datalist id="import-known-sections">
        <option v-for="section in knownSections" :key="section" :value="section" />
      </datalist>
      <button
        type="button"
        :disabled="store.busy || !importDir.trim()"
        @click="scanImport"
      >
        扫描（不写文件）
      </button>

      <template v-if="candidates">
        <p class="build__muted">
          找到 {{ candidates.length }} 篇，可导入 {{ importable }} 篇；
          目标已存在的会被跳过，不覆盖。
        </p>
        <ul class="settings__import-list">
          <li v-for="candidate in candidates" :key="candidate.source" :class="{ skip: !candidate.importable }">
            <span>
              <code>{{ candidate.source }}</code> →
              <code>{{ candidate.target }}</code>
            </span>
            <span v-if="!candidate.importable" class="badge badge--seo-warn">已存在</span>
            <span v-for="warning in candidate.warnings" :key="warning" class="build__muted">
              {{ warning }}
            </span>
          </li>
        </ul>
        <div class="settings__import-row">
          <button
            type="button"
            class="btn--primary"
            :disabled="store.busy || importable === 0"
            @click="runImport"
          >
            导入 {{ importable }} 篇
          </button>
          <button type="button" @click="candidates = null">收起</button>
        </div>
      </template>
    </div>

    <div v-if="section !== 'import'" class="settings__save">
      <button type="button" class="btn--primary" :disabled="store.busy" @click="save">
        保存设置
      </button>
      <span class="build__muted">保存的是整份 staticsmith.toml，不只当前这一段。</span>
    </div>
  </section>
</template>

