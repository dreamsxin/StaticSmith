<script setup lang="ts">
/** 站点设置：编辑 staticsmith.toml 的可视化表单。 */
import { reactive, watch } from 'vue'

import type { SiteConfig } from '../api'
import { actions, store } from '../store'

/** 表单持有一份可变副本，保存时才写回磁盘。 */
const form = reactive<SiteConfig>(clone(store.project?.config))

watch(
  () => store.project?.config,
  (config) => Object.assign(form, clone(config)),
)

function clone(config: SiteConfig | null | undefined): SiteConfig {
  if (config) return JSON.parse(JSON.stringify(config)) as SiteConfig
  return {
    site: { title: '', description: '', base_url: '', language: 'zh-CN', extra: {} },
    build: {
      output_dir: './dist',
      content_dir: './content',
      theme_dir: './themes/default',
      template_dir: './templates',
      page_size: 10,
      minify: true,
    },
    deploy: { type: 'none', git: null, ftp: null },
  }
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
      <label>输出目录<input v-model="form.build.output_dir" type="text" /></label>
      <label>每页条数<input v-model.number="form.build.page_size" type="number" min="1" /></label>
      <label class="settings__checkbox">
        <input v-model="form.build.minify" type="checkbox" />
        压缩输出 HTML
      </label>
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

      <button type="button" :disabled="store.busy" @click="actions.saveConfig(form)">
        保存设置
      </button>
    </div>
  </section>
</template>
