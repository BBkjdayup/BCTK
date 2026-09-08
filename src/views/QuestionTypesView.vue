<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { ArrowDown, ArrowUp, Edit, Plus, Delete } from '@element-plus/icons-vue'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import type {
  QuestionTypeBehavior,
  QuestionTypeDefinition,
  SaveQuestionTypeRequest,
} from '../types/domain'
import { matchesChoiceBehavior } from '../utils/questionTypes'

const appStore = useAppStore()
const dialogVisible = ref(false)
const saving = ref(false)
const reordering = ref(false)
const editing = ref<QuestionTypeDefinition | null>(null)

const form = reactive({
  name: '',
  behavior: 'open_response' as QuestionTypeBehavior,
  aliasesText: '',
  defaultOptionsText: '',
  isEnabled: true,
})

const types = computed(() => [...appStore.questionTypes].sort((left, right) => (
  left.sortOrder - right.sortOrder || left.code.localeCompare(right.code)
)))

const behaviorOptions: Array<{ value: QuestionTypeBehavior, label: string, description: string }> = [
  { value: 'single_choice', label: '单选结构', description: '显示多个选项，只能选择一个答案' },
  { value: 'multiple_choice', label: '多选结构', description: '显示多个选项，可以选择多个答案' },
  { value: 'fill_blank', label: '填写答案结构', description: '没有选项，适合填空、名词解释等短答案' },
  { value: 'open_response', label: '开放作答结构', description: '没有选项，适合简答、计算、案例分析等长答案' },
]

function behaviorLabel(behavior: QuestionTypeBehavior) {
  return behaviorOptions.find((item) => item.value === behavior)?.label ?? behavior
}

function openCreate() {
  editing.value = null
  Object.assign(form, {
    name: '',
    behavior: 'open_response',
    aliasesText: '',
    defaultOptionsText: '',
    isEnabled: true,
  })
  dialogVisible.value = true
}

function openEdit(definition: QuestionTypeDefinition) {
  editing.value = definition
  Object.assign(form, {
    name: definition.name,
    behavior: definition.behavior,
    aliasesText: definition.aliases.join('\n'),
    defaultOptionsText: definition.defaultOptions.join('\n'),
    isEnabled: definition.isEnabled,
  })
  dialogVisible.value = true
}

function splitLines(value: string) {
  return [...new Set(value.split(/[\n,，、]+/u).map((item) => item.trim()).filter(Boolean))]
}

async function save() {
  if (!form.name.trim()) {
    ElMessage.warning('请填写题型名称')
    return
  }
  const request: SaveQuestionTypeRequest = {
    code: editing.value?.code ?? null,
    name: form.name.trim(),
    behavior: form.behavior,
    aliases: splitLines(form.aliasesText),
    defaultOptions: matchesChoiceBehavior(form.behavior) ? splitLines(form.defaultOptionsText) : [],
    isEnabled: form.isEnabled,
  }
  saving.value = true
  try {
    await appStore.saveQuestionType(request)
    dialogVisible.value = false
    ElMessage.success(editing.value ? '题型设置已保存' : '新题型已创建')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '题型保存失败'))
  } finally {
    saving.value = false
  }
}

async function toggleEnabled(definition: QuestionTypeDefinition, enabled: boolean) {
  try {
    await appStore.saveQuestionType({
      code: definition.code,
      name: definition.name,
      behavior: definition.behavior,
      aliases: definition.aliases,
      defaultOptions: definition.defaultOptions,
      isEnabled: enabled,
    })
    ElMessage.success(enabled ? '题型已启用' : '题型已停用，已有题目仍会保留')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '题型状态修改失败'))
  }
}

async function remove(definition: QuestionTypeDefinition) {
  try {
    await ElMessageBox.confirm(
      `确定删除题型“${definition.name}”吗？删除后无法恢复。`,
      '删除题型',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
    await appStore.deleteQuestionType(definition.code)
    ElMessage.success('题型已删除')
  } catch (reason) {
    if (reason === 'cancel' || reason === 'close') return
    ElMessage.error(errorMessage(reason, '题型删除失败'))
  }
}

async function move(index: number, direction: -1 | 1) {
  const target = index + direction
  if (target < 0 || target >= types.value.length) return
  const ordered = [...types.value]
  const [item] = ordered.splice(index, 1)
  if (!item) return
  ordered.splice(target, 0, item)
  reordering.value = true
  try {
    await appStore.saveQuestionTypeOrder(ordered.map((definition, position) => ({
      id: definition.code,
      sortOrder: (position + 1) * 10,
    })))
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '题型排序保存失败'))
  } finally {
    reordering.value = false
  }
}

onMounted(() => {
  if (!appStore.questionTypes.length) void appStore.refreshTaxonomy()
})
</script>

<template>
  <section class="page-main question-types-page">
      <el-alert
        type="info"
        :closable="false"
        show-icon
        title="题型名称决定界面和试卷中的显示文字；基础答题结构决定是否有选项以及答案填写方式。"
      />

      <section class="surface types-card" v-loading="reordering">
        <div class="types-table types-table--header">
          <span>顺序</span><span>题型</span><span>基础结构</span><span>Word 识别别名</span><span>使用情况</span><span>状态</span>
          <div class="types-header-actions">
            <span>操作</span>
            <el-button type="primary" size="small" :icon="Plus" @click="openCreate">添加题型</el-button>
          </div>
        </div>
        <div v-for="(definition, index) in types" :key="definition.code" class="types-table types-table--row">
          <div class="order-actions">
            <el-button text :icon="ArrowUp" :disabled="index === 0" aria-label="上移" @click="move(index, -1)" />
            <el-button text :icon="ArrowDown" :disabled="index === types.length - 1" aria-label="下移" @click="move(index, 1)" />
          </div>
          <div class="type-name">
            <strong>{{ definition.name }}</strong>
            <el-tag v-if="definition.isBuiltin" size="small" effect="plain">内置</el-tag>
            <small>{{ definition.code }}</small>
          </div>
          <span>{{ behaviorLabel(definition.behavior) }}</span>
          <div class="alias-list">
            <el-tag v-for="alias in definition.aliases" :key="alias" size="small" effect="plain">{{ alias }}</el-tag>
            <span v-if="!definition.aliases.length" class="muted">未设置</span>
          </div>
          <span>{{ definition.questionCount }} 道题 · {{ definition.paperItemCount }} 个试卷快照</span>
          <el-switch
            :model-value="definition.isEnabled"
            :aria-label="`${definition.name}启用状态`"
            @change="toggleEnabled(definition, Boolean($event))"
          />
          <div class="row-actions">
            <el-button text type="primary" :icon="Edit" @click="openEdit(definition)">编辑</el-button>
            <el-button
              v-if="!definition.isBuiltin"
              text
              type="danger"
              :icon="Delete"
              :disabled="definition.questionCount + definition.paperItemCount > 0"
              @click="remove(definition)"
            >删除</el-button>
          </div>
        </div>
      </section>
    <el-dialog v-model="dialogVisible" :title="editing ? '编辑题型' : '添加题型'" width="620px" destroy-on-close>
      <el-form label-position="top">
        <el-form-item label="题型名称" required>
          <el-input v-model="form.name" maxlength="40" show-word-limit placeholder="例如：名词解释题、计算题" />
        </el-form-item>
        <el-form-item label="基础答题结构" required>
          <el-select
            v-model="form.behavior"
            class="full-width"
            :disabled="Boolean(editing?.isBuiltin || (editing && editing.questionCount + editing.paperItemCount > 0))"
          >
            <el-option v-for="option in behaviorOptions" :key="option.value" :value="option.value" :label="option.label">
              <div class="behavior-option"><strong>{{ option.label }}</strong><small>{{ option.description }}</small></div>
            </el-option>
          </el-select>
        </el-form-item>
        <el-form-item label="Word 识别别名">
          <el-input
            v-model="form.aliasesText"
            type="textarea"
            :rows="3"
            placeholder="每行一个，例如：正误题&#10;是非题"
          />
          <div class="form-help">Word 中出现这些大题标题时，会自动识别为当前题型。</div>
        </el-form-item>
        <el-form-item v-if="matchesChoiceBehavior(form.behavior)" label="默认选项（可选）">
          <el-input
            v-model="form.defaultOptionsText"
            type="textarea"
            :rows="3"
            placeholder="判断题可以填写：&#10;正确&#10;错误"
          />
        </el-form-item>
        <el-form-item label="启用">
          <el-switch v-model="form.isEnabled" />
          <span class="inline-help">停用后不能再新建该题型，但已有题目和历史试卷不会删除。</span>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="save">保存</el-button>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.question-types-page { overflow: auto; }
.page-main { min-width: 980px; padding: 24px 28px 40px; }
.types-card { margin-top: 16px; overflow: hidden; }
.types-table { display: grid; grid-template-columns: 68px 150px 110px minmax(150px, 1fr) 165px 64px 156px; align-items: center; gap: 12px; padding: 14px 18px; }
.types-table--header { border-bottom: 1px solid var(--border); background: #f8fafc; color: #64748b; font-size: 13px; font-weight: 700; }
.types-header-actions { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
.types-table--row { min-height: 84px; border-bottom: 1px solid #edf1f6; color: #334155; }
.types-table--row:last-child { border-bottom: 0; }
.order-actions, .row-actions, .alias-list { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; }
.order-actions :deep(.el-button + .el-button) { margin-left: 0; }
.type-name { min-width: 0; display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
.type-name small { width: 100%; overflow: hidden; color: #94a3b8; text-overflow: ellipsis; }
.muted, .form-help, .inline-help { color: #94a3b8; font-size: 12px; }
.full-width { width: 100%; }
.behavior-option { display: flex; justify-content: space-between; gap: 20px; }
.behavior-option small { color: #94a3b8; }
.inline-help { margin-left: 10px; }
</style>
