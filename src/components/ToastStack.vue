<script setup lang="ts">
/**
 * 操作反馈通知。
 *
 * 之前失败信息只写进 `store.error`，而多数调用点不显示它——用户看到的就是「点了没反应」。
 * 现在所有成功与失败都在这里冒出来，右下角堆叠，可手动关闭。
 */
import { actions, store } from '../store'
</script>

<template>
  <div class="toasts" role="status" aria-live="polite">
    <div
      v-for="toast in store.toasts"
      :key="toast.id"
      class="toast"
      :class="`toast--${toast.kind}`"
    >
      <span class="toast__text">{{ toast.message }}</span>
      <button type="button" class="toast__close" title="关闭" @click="actions.dismissToast(toast.id)">
        ×
      </button>
    </div>
  </div>
</template>
