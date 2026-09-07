<script setup lang="ts">
/**
 * 预览面板，两种模式：
 *
 * - **内存预览**（默认）：后端用同一条渲染路径渲出完整 HTML 灌进 iframe 的 `srcdoc`。
 *   保存即可见，但 iframe 没有文件访问权限，图片与 CSS 取不到。
 * - **本地服务器**：起一个只监听 127.0.0.1 的静态服务器指向产物目录，预览与线上完全一致，
 *   代价是需要先生成一次。
 *
 * 标签页、分页页这类非内容产物没有对应的源文件，只能走服务器模式，
 * 由 `store.previewTarget` 指定要看哪个地址。
 */
import { computed } from 'vue'

import { actions, store } from '../store'

/** 当前编辑页在站点里的地址。 */
const pageUrl = computed(
  () => store.project?.pages.find((p) => p.source === store.currentSource)?.url ?? '/',
)

/** 预览的目标地址：优先产物地址，否则当前编辑页。 */
const targetUrl = computed(() => store.previewTarget ?? pageUrl.value)

/**
 * 服务器预览地址。
 *
 * 不靠改 URL 顶掉重载：预览服务器会在 HTML 响应里注入一段轮询脚本，
 * 产物变了页面自己刷新，并把滚动位置带回来——写长文时最烦的就是每次生成都跳回顶部。
 */
const serverPageUrl = computed(() =>
  store.previewServer ? `${store.previewServer}${targetUrl.value}` : null,
)



</script>

<template>
  <section class="preview">
    <header class="preview__bar">
      <span>{{ store.previewServer ? '本地服务器预览' : '布局继承预览' }}</span>
      <code v-if="store.previewTarget" class="preview__target">{{ store.previewTarget }}</code>
      <span class="editor__spacer" />
      <button
        v-if="store.previewTarget"
        type="button"
        @click="actions.clearPreviewTarget()"
      >
        返回当前文章
      </button>
      <button type="button" :disabled="store.busy" @click="actions.togglePreviewServer()">
        {{ store.previewServer ? '停止服务器' : '启动本地服务器' }}
      </button>
      <label class="preview__auto" title="保存后自动增量生成，产物与服务器预览随之更新">
        <input
          type="checkbox"
          :checked="store.autoBuild"
          @change="actions.setAutoBuild(($event.target as HTMLInputElement).checked)"
        />
        保存即生成
      </label>
      <button
        v-if="serverPageUrl"
        type="button"
        :disabled="store.busy"
        @click="actions.openInBrowser(serverPageUrl)"
      >
        在浏览器打开
      </button>
    </header>

    <iframe
      v-if="serverPageUrl"
      class="preview__frame"
      title="本地服务器预览"
      :src="serverPageUrl"
    />

    <iframe
      v-else-if="store.previewHtml"
      class="preview__frame"
      title="页面预览"
      sandbox="allow-same-origin"
      :srcdoc="store.previewHtml"
    />
    <p v-else class="preview__empty">保存或刷新后在此显示最终呈现效果。</p>

    <footer v-if="store.previewServer" class="editor__foot">
      {{ store.previewServer }} · 仅监听回环地址{{
        store.autoBuild ? '，保存后自动重新生成' : '，需先「生成」才能看到最新产物'
      }}
    </footer>
    <footer v-else class="editor__foot">
      内存预览不加载图片与 CSS；需要完整效果请启动本地服务器
    </footer>
  </section>
</template>
