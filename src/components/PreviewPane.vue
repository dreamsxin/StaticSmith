<script setup lang="ts">
/**
 * 布局继承预览。
 *
 * 直接把后端渲染好的完整 HTML 灌进 iframe：预览与最终产物走的是同一条渲染路径，
 * 因此「所见」必然等于「所得」。iframe 加 sandbox，避免站点脚本影响宿主界面。
 */
import { store } from '../store'
</script>

<template>
  <section class="preview">
    <header class="preview__bar">布局继承预览</header>
    <iframe
      v-if="store.previewHtml"
      class="preview__frame"
      title="页面预览"
      sandbox="allow-same-origin"
      :srcdoc="store.previewHtml"
    />
    <p v-else class="preview__empty">保存或刷新后在此显示最终呈现效果。</p>
  </section>
</template>
