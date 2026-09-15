<script setup lang="ts">
/**
 * 多选之后的批量动作条：改标签、发布 / 设为草稿、移动到栏目、删除。
 *
 * 「把这十二篇都补上标签」「这一批放出去」是运营里最费手的操作，逐篇点开必然出错。
 * 从 `PageList.vue` 抽出来的第三块（前两块是栏目头与跨文件替换），理由一样：
 * 这里有两条会**改地址**或**不可逆**的路径，各带一个干跑确认态，而确认态的出口
 * 是最容易漏的地方。
 *
 * **选择态留在上层**：勾选框长在文章列表的每一行上，与这条动作条不在同一棵子树里。
 * 所以这里只收 `selected`（已勾的源路径），并在动作做完后 emit `clear` 请上层清空——
 * 文件可能已经改名、搬走或删掉，旧的选中集没有意义。
 */
import { ref } from 'vue'

import type { BatchPreview } from '../api'
import { actions, store } from '../store'
import { parseList } from '../text'

const props = defineProps<{
  /** 已勾选的源路径。 */
  selected: readonly string[]
  /** 站内已有的标签，给输入框做候选：避免同一个概念写出三种写法。 */
  knownTags: readonly string[]
}>()

const emit = defineEmits<{
  /** 选中当前可见（已按搜索与筛选过滤）的全部条目。 */
  selectVisible: []
  /** 清空选择。批量动作做完也会发它。 */
  clear: []
}>()

const tags = ref('')
const section = ref('')
const keepAliases = ref(true)

/**
 * 待确认的动作及其干跑结果。
 *
 * 搬动会改地址、删除不可逆，这两件事先看一眼「哪几篇会怎么变」再落盘；
 * 加标签、切草稿反手就能改回来，不值得多一次点击。
 */
const pending = ref<{ kind: 'move' | 'delete'; preview: BatchPreview } | null>(null)

function reset() {
  pending.value = null
  emit('clear')
}

async function applyTags(mode: 'add' | 'remove') {
  const list = parseList(tags.value)
  if (!list.length) return
  const [add, remove] = mode === 'add' ? [list, []] : [[], list]
  await actions.batchEditTags([...props.selected], add, remove)
  tags.value = ''
  reset()
}

async function setDraft(draft: boolean) {
  await actions.batchSetDraft([...props.selected], draft)
  reset()
}

/** 先干跑：让用户看清「哪几篇会搬到哪、旧地址是什么」再决定。 */
async function previewMove() {
  const preview = await actions.batchPreview([...props.selected], {
    kind: 'move',
    to_section: section.value.trim(),
  })
  if (preview) pending.value = { kind: 'move', preview }
}

async function previewDelete() {
  const preview = await actions.batchPreview([...props.selected], { kind: 'delete' })
  if (preview) pending.value = { kind: 'delete', preview }
}

async function confirmPending() {
  const kind = pending.value?.kind
  pending.value = null
  if (kind === 'move') {
    await actions.batchMove([...props.selected], section.value.trim(), keepAliases.value)
    section.value = ''
  } else if (kind === 'delete') {
    await actions.batchDelete([...props.selected])
  }
  reset()
}
</script>

<template>
  <div class="page-list__batch">
    <p class="page-list__batch-head">
      已选 {{ props.selected.length }} 篇
      <span class="page-list__spacer" />
      <button type="button" class="page-list__icon" @click="emit('selectVisible')">全选当前</button>
      <button type="button" class="page-list__icon" @click="reset">清空</button>
    </p>

    <template v-if="props.selected.length">
      <datalist id="batch-known-tags">
        <option v-for="tag in props.knownTags" :key="tag" :value="tag" />
      </datalist>

      <div class="page-list__batch-row">
        <input
          v-model="tags"
          type="text"
          list="batch-known-tags"
          placeholder="标签，逗号分隔"
          aria-label="批量标签"
        />
        <button
          type="button"
          :disabled="store.busy || !tags.trim()"
          title="加到每篇（原有标签保留）"
          @click="applyTags('add')"
        >
          加
        </button>
        <button
          type="button"
          :disabled="store.busy || !tags.trim()"
          title="从每篇去掉"
          @click="applyTags('remove')"
        >
          去
        </button>
      </div>

      <div class="page-list__batch-row">
        <input
          v-model="section"
          type="text"
          list="known-sections"
          placeholder="移动到栏目（留空为根目录）"
          aria-label="目标栏目"
        />
        <button type="button" :disabled="store.busy" @click="previewMove">移动…</button>
      </div>
      <label class="page-list__keep">
        <input v-model="keepAliases" type="checkbox" />
        移动后保留旧地址（生成重定向页）
      </label>

      <div class="page-list__batch-row">
        <button type="button" :disabled="store.busy" @click="setDraft(false)">发布</button>
        <button type="button" :disabled="store.busy" @click="setDraft(true)">设为草稿</button>
        <span class="page-list__spacer" />
        <button
          type="button"
          class="page-list__icon"
          title="删除选中的内容，不可撤销"
          @click="previewDelete"
        >
          删除…
        </button>
      </div>

      <!-- 干跑结果：搬动与删除先看清「哪几篇会怎么变」再落盘 -->
      <div v-if="pending" class="page-list__dry">
        <p class="page-list__batch-head">
          {{ pending.kind === 'move' ? '将搬动' : '将删除' }}
          {{ pending.preview.affected }} / {{ pending.preview.changes.length }} 篇
        </p>
        <ul class="page-list__dry-list">
          <li
            v-for="change in pending.preview.changes"
            :key="change.source"
            :class="{ skip: !change.changes }"
          >
            <code>{{ change.source }}</code>
            <span>{{ change.effect }}</span>
          </li>
        </ul>
        <!-- 搬动会写到用户没勾的文件上（改它们里面的链接），必须先说清楚：
             背着人改东西比不改更糟 -->
        <p v-if="pending.preview.refs.length" class="page-list__batch-note">
          另会把 {{ pending.preview.refs.length }} 篇里的
          {{ pending.preview.refs.reduce((sum, item) => sum + item.hits, 0) }}
          处站内链接改到新地址：{{ pending.preview.refs.map((item) => item.source).join('、') }}
        </p>
        <div class="page-list__batch-row">
          <button
            type="button"
            :class="pending.kind === 'delete' ? 'page-list__danger' : 'btn--primary'"
            :disabled="store.busy || pending.preview.affected === 0"
            @click="confirmPending"
          >
            {{ pending.kind === 'move' ? '确认移动' : '确认删除' }}
            {{ pending.preview.affected }} 篇
          </button>
          <button type="button" @click="pending = null">取消</button>
        </div>
      </div>
    </template>
    <p v-else class="page-list__hint">勾选左侧条目，或点「全选当前」。</p>
  </div>
</template>
