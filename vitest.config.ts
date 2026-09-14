import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

// 单独一份配置，不往 vite.config.ts 里塞 test 段：那份只管「怎么打包给 Tauri」，
// 混进测试设置之后两件事会互相牵制（例如为了测试改 build.target）。
//
// **默认环境仍是 node**：大多数测试的对象是纯函数（着色器、文本解析、大纲、链接拼接），
// 它们不该为了别人的 DOM 付启动开销。要 DOM 的组件测试在文件顶部写一行
// `// @vitest-environment jsdom` 自己声明——谁用谁付，也一眼看出这个文件在测什么层。
//
// 需要 `@vitejs/plugin-vue`：不挂它，`.vue` 单文件组件在测试里根本编译不了。
export default defineConfig({
  plugins: [vue()],
  test: {
    include: ['src/**/*.test.ts'],
  },
})
