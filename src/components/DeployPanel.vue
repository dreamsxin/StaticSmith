<script setup lang="ts">
/**
 * 发布面板。
 *
 * 凭证（Git Token / FTP 密码）只通过 `save_secret` 交给 Rust 侧写入系统凭据管理器，
 * 不进 `staticsmith.toml`，也不留在前端状态里——保存后立即清空输入框。
 */
import { computed, ref, watch } from 'vue'

import { ftpOverwriteLabel } from '../labels'
import { store, actions } from '../store'

const secret = ref('')
/** 凭据管理器里是否已有这条凭证。null 表示还没查。 */
const stored = ref<boolean | null>(null)

const deploy = computed(() => store.project?.config.deploy)

/**
 * 凭据条目名向 Rust 侧问，不在这里拼。
 *
 * 命名规则（`git:<remote>` / `ftp:<用户>@<主机>`）只在 `account_for_config` 一处定义：
 * 两边各写一份的话，改了规则之后会出现「保存写进 A、查询读的是 B」，
 * 用户看到的是「明明保存过，界面说没有」——这类不一致最难被发现。
 *
 * `null` 表示问不出来（命令报错），与「配置里没选发布方式」得到的空串是两回事：
 * 前者要说明出了错，后者要指路去设置页。
 */
const account = ref<string | null>('')
const accountFailed = computed(() => account.value === null)

/**
 * 发布配置变了就重新问一次条目名，并跟着查一次凭证在不在。
 *
 * 之前只有「保存成功」的一次性提示，重开应用后完全看不出凭证在不在，
 * 用户只能靠「测试连接」间接猜——所以这里把状态显式摆出来。
 */
watch(
  deploy,
  async () => {
    account.value = store.project ? await actions.deployAccount() : ''
    stored.value = account.value ? await actions.hasSecret(account.value) : null
  },
  { immediate: true, deep: true },
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
        <li>方式：{{ deploy.type === 'git' ? 'Git 仓库' : 'FTP（明文）' }}</li>
        <li v-if="deploy.git">远端：{{ deploy.git.remote }}（分支 {{ deploy.git.branch }}）</li>
        <li v-if="deploy.ftp">
          主机：{{ deploy.ftp.host }}:{{ deploy.ftp.port }} → {{ deploy.ftp.remote_path }}
        </li>
        <!-- 规则的后果落在这一页（「跳过 N 个」是它决定的），所以这里也要看得见，
             不能只在设置页选完就不再露面 -->
        <li v-if="deploy.ftp">远端已存在时：{{ ftpOverwriteLabel(deploy.ftp.overwrite) }}</li>
      </ul>

      <h3>凭证</h3>
      <p v-if="accountFailed" class="warn">
        问不出凭据条目名（上一条错误通知里有原因），所以下面的保存与删除都用不了。
        这不代表凭证没存过——请重试或检查系统凭据管理器是否可用。
      </p>
      <p v-else class="build__muted">
        凭据条目：<code>{{ account || '（未配置）' }}</code>
      </p>
      <p v-if="account">
        <span v-if="stored === null" class="badge badge--draft">状态未知</span>
        <span v-else-if="stored" class="badge badge--ok">已存储</span>
        <span v-else class="badge badge--draft">未存储</span>
        <span class="build__muted">
          {{
            stored === null
              ? '查不到凭据管理器里的状态，仍可尝试保存或直接发布。'
              : stored
                ? '发布时直接从系统凭据管理器读取，可重新输入覆盖。'
                : '发布前需要先保存 Token / 密码。'
          }}
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
