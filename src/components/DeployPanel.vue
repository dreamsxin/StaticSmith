<script setup lang="ts">
/**
 * 发布面板。
 *
 * 凭证（Git Token / FTP 密码）只通过 `save_secret` 交给 Rust 侧写入系统凭据管理器，
 * 不进 `staticsmith.toml`，也不留在前端状态里——保存后立即清空输入框。
 */
import { computed, ref, watch } from 'vue'

import { store, actions } from '../store'

const secret = ref('')
/** 凭据管理器里是否已有这条凭证。null 表示还没查。 */
const stored = ref<boolean | null>(null)

const deploy = computed(() => store.project?.config.deploy)

/** 凭据条目名必须与 Rust 侧 `account_for_git` / `account_for_ftp` 保持一致。 */
const account = computed(() => {
  const d = deploy.value
  if (!d) return ''
  if (d.type === 'git' && d.git) return `git:${d.git.remote}`
  if (d.type === 'ftp' && d.ftp) return `ftp:${d.ftp.username}@${d.ftp.host}`
  return ''
})

/**
 * 跟着条目名查一次状态。
 *
 * 之前只有「保存成功」的一次性提示，重开应用后完全看不出凭证在不在，
 * 用户只能靠「测试连接」间接猜——所以这里把状态显式摆出来。
 */
watch(
  account,
  async (value) => {
    stored.value = value ? await actions.hasSecret(value) : null
  },
  { immediate: true },
)

async function storeSecret() {
  if (!account.value || !secret.value) return
  await actions.saveSecret(account.value, secret.value)
  secret.value = ''
  stored.value = await actions.hasSecret(account.value)
}

async function removeSecret() {
  if (!account.value) return
  await actions.deleteSecret(account.value)
  stored.value = await actions.hasSecret(account.value)
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
      <p v-if="account">
        <span v-if="stored" class="badge badge--ok">已存储</span>
        <span v-else class="badge badge--draft">未存储</span>
        <span class="build__muted">
          {{ stored ? '发布时直接从系统凭据管理器读取，可重新输入覆盖。' : '发布前需要先保存 Token / 密码。' }}
        </span>
      </p>
      <label>
        Token / 密码
        <input v-model="secret" type="password" autocomplete="off" :disabled="!account" />
      </label>
      <div class="deploy__actions">
        <button type="button" :disabled="!account || !secret" @click="storeSecret">
          {{ stored ? '覆盖已存储的凭证' : '保存到系统凭据管理器' }}
        </button>
        <button type="button" :disabled="!stored || store.busy" @click="removeSecret">
          删除凭证
        </button>
        <button type="button" :disabled="store.busy" @click="actions.checkDeploy()">
          测试连接
        </button>
      </div>
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
      <!-- 未发布时不让结果区整块消失（见 ui-design.md 6.9）：消失让人以为功能不存在 -->
      <p v-else class="empty-hint">
        本次会话还没发布过。发布只上传 <code>dist/</code> 里变化的文件，
        完成后这里会列出目标、上传与跳过的数量、耗时，Git 方式还会给出提交号。
      </p>
    </div>
  </section>
</template>
