<script setup lang="ts">
/**
 * 发布面板。
 *
 * 凭证（Git Token / FTP 密码）只通过 `save_secret` 交给 Rust 侧写入系统凭据管理器，
 * 不进 `staticsmith.toml`，也不留在前端状态里——保存后立即清空输入框。
 */
import { computed, ref } from 'vue'

import { store, actions } from '../store'

const secret = ref('')
const saved = ref(false)

const deploy = computed(() => store.project?.config.deploy)

/** 凭据条目名必须与 Rust 侧 `account_for_git` / `account_for_ftp` 保持一致。 */
const account = computed(() => {
  const d = deploy.value
  if (!d) return ''
  if (d.type === 'git' && d.git) return `git:${d.git.remote}`
  if (d.type === 'ftp' && d.ftp) return `ftp:${d.ftp.username}@${d.ftp.host}`
  return ''
})

async function storeSecret() {
  if (!account.value || !secret.value) return
  await actions.saveSecret(account.value, secret.value)
  secret.value = ''
  saved.value = true
}
</script>

<template>
  <section class="deploy">
    <div class="deploy__panel">
      <h3>发布目标</h3>
      <p v-if="!deploy || deploy.type === 'none'">
        尚未配置发布方式，请在「设置」中选择 Git 或 FTP。
      </p>
      <ul v-else class="build__stats">
        <li>方式：{{ deploy.type === 'git' ? 'Git 仓库' : (deploy.ftp?.sftp ? 'SFTP' : 'FTP') }}</li>
        <li v-if="deploy.git">远端：{{ deploy.git.remote }}（分支 {{ deploy.git.branch }}）</li>
        <li v-if="deploy.ftp">
          主机：{{ deploy.ftp.host }}:{{ deploy.ftp.port }} → {{ deploy.ftp.remote_path }}
        </li>
      </ul>

      <h3>凭证</h3>
      <p class="build__muted">
        凭据条目：<code>{{ account || '（未配置）' }}</code>
      </p>
      <label>
        Token / 密码
        <input v-model="secret" type="password" autocomplete="off" :disabled="!account" />
      </label>
      <div class="deploy__actions">
        <button type="button" :disabled="!account || !secret" @click="storeSecret">
          保存到系统凭据管理器
        </button>
        <button type="button" :disabled="store.busy" @click="actions.checkDeploy()">
          测试连接
        </button>
      </div>
      <p v-if="saved" class="build__muted">凭证已写入系统凭据管理器。</p>
    </div>

    <div class="deploy__panel">
      <h3>执行发布</h3>
      <button type="button" :disabled="store.busy" @click="actions.deploy()">一键发布</button>

      <template v-if="store.lastDeploy">
        <ul class="build__stats">
          <li>目标：{{ store.lastDeploy.target }}</li>
          <li>上传：{{ store.lastDeploy.uploaded.length }} 个文件</li>
          <li>跳过（未变化）：{{ store.lastDeploy.skipped }}</li>
          <li v-if="store.lastDeploy.commit">提交：{{ store.lastDeploy.commit }}</li>
          <li>耗时：{{ store.lastDeploy.duration_ms }} ms</li>
        </ul>
        <p v-for="warning in store.lastDeploy.warnings" :key="warning" class="warn">
          {{ warning }}
        </p>
      </template>
    </div>
  </section>
</template>
