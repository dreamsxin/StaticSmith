<script setup lang="ts">
/** 站点设置：编辑 staticsmith.toml 的可视化表单。 */
import { computed, reactive, watch } from 'vue'

import type { SiteConfig } from '../api'
import { actions, store } from '../store'

/** 表单持有一份可变副本，保存时才写回磁盘。 */
const form = reactive<SiteConfig>(clone(store.project?.config))

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
 * 按维度数量决定写回哪种形状，然后保存。
 *
 * 不改 `form` 本身：保存可能被后端校验驳回（比如两个维度用了同一个 slug），
 * 那时界面上的行必须还在，否则用户刚填的东西看起来凭空少了一条。
 */
function save() {
  const rows = form.taxonomies.map((row) => ({ ...row, enabled: true }))
  const payload = clone(form)
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
    <div class="settings__panel">
      <h3>站点</h3>
      <label>标题<input v-model="form.site.title" type="text" /></label>
      <label>描述<input v-model="form.site.description" type="text" /></label>
      <label>站点地址<input v-model="form.site.base_url" type="text" /></label>
      <label>语言<input v-model="form.site.language" type="text" /></label>

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


    <div class="settings__panel">
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

      <button type="button" :disabled="store.busy" @click="save">保存设置</button>
    </div>
  </section>
</template>
