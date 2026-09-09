import { defineConfig } from 'vitest/config'

// 单独一份配置，不往 vite.config.ts 里塞 test 段：那份只管「怎么打包给 Tauri」，
// 混进测试设置之后两件事会互相牵制（例如为了测试改 build.target）。
//
// environment 用默认的 node：当前这批测试的对象都是纯函数（着色器、文本解析），
// 不碰 DOM。等要测 useSplit 或组件时再加 jsdom 与 @vue/test-utils——
// 那两个依赖只有到那时才真的需要，提前装等于先付账。
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
  },
})
