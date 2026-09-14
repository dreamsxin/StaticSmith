<script setup lang="ts">
/**
 * 操作反馈通知。
 *
 * 之前失败信息只写进 `store.error`，而多数调用点不显示它——用户看到的就是「点了没反应」。
 * 现在所有成功与失败都在这里冒出来，右下角堆叠，可手动关闭。
 *
 * 几秒后自动消失，事后要再读一遍去状态栏的「消息」（`NoticeCenter`）。
 */
import { actions, store } from '../store'
</script>

<template>
  <div class="toasts">
    <!-- 每条自己是一个 live region，因为程度不同该有不同的抢读策略：
         错误用 alert（assertive，会打断读屏器当前朗读——用户需要立刻知道操作失败了），
         成功与提示用 status（polite，等它读完手上的内容）。
         整个容器统一 polite 的话，错误就只能排队，那条最该被听到的反而最晚到。 -->
    <div
      v-for="toast in store.toasts"
      :key="toast.id"
      class="toast"
      :class="`toast--${toast.kind}`"
      :role="toast.kind === 'error' ? 'alert' : 'status'"
      :aria-live="toast.kind === 'error' ? 'assertive' : 'polite'"
    >
      <span class="toast__text">{{ toast.message }}</span>
      <button
        type="button"
        class="toast__close"
        title="关闭"
        aria-label="关闭这条通知"
        @click="actions.dismissToast(toast.id)"
      >
        ×
      </button>
    </div>
  </div>
</template>
