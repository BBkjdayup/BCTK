<script setup lang="ts">
import { ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { CopyDocument, Refresh } from '@element-plus/icons-vue'
import { miniCloud } from '../services/miniProgramCloud'
import { defaultSupportContact, parseSupportContact } from '../services/supportContact'

const props = defineProps<{ modelValue: boolean; account: string; live: boolean; refreshing?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [boolean]; refresh: [] }>()
const contact = ref({ ...defaultSupportContact })
const loading = ref(false)
const error = ref('')
let generation = 0
async function load() {
  const request = ++generation
  error.value = ''
  if (!props.live) return
  loading.value = true
  try {
    const result = parseSupportContact(await miniCloud.request('GET', '/support-contact'))
    if (request === generation) contact.value = result
  } catch {
    if (request === generation) error.value = '暂时无法更新联系方式，当前显示已保存的信息。'
  } finally { if (request === generation) loading.value = false }
}
watch(() => props.modelValue, open => { if (open) void load(); else { generation++; loading.value = false } }, { immediate: true })
async function copy(value: string, label: string) {
  try { await navigator.clipboard.writeText(value); ElMessage.success(`${label}已复制`) }
  catch { ElMessage.warning('复制失败，请选中文字后手动复制') }
}
</script>

<template>
  <el-dialog :model-value="modelValue" title="开通 / 续费" width="620px" align-center class="support-contact-dialog" @update:model-value="emit('update:modelValue', $event)">
    <div v-if="error" class="support-contact-error" role="alert">{{ error }}<el-button link type="primary" :loading="loading" @click="load">重试</el-button></div>
    <div class="support-contact-content" :aria-busy="loading">
      <div v-if="contact.qr_code" class="support-contact-qr"><img :src="contact.qr_code" alt="客服微信添加好友二维码"></div>
      <div class="support-contact-details">
        <h3>{{ contact.display_name }}</h3>
        <div class="support-contact-field"><span>客服微信</span><strong>{{ contact.wechat_id }}</strong><el-button :icon="CopyDocument" @click="copy(contact.wechat_id, '微信号')">复制微信号</el-button></div>
        <div class="support-contact-field"><span>软件账号</span><strong>{{ account || (live ? '请先登录小程序管理账号' : '本地界面预览') }}</strong><el-button v-if="account" :icon="CopyDocument" @click="copy(account, '软件账号')">复制账号</el-button></div>
        <div v-if="contact.service_hours" class="support-contact-field"><span>联系时间</span><strong>{{ contact.service_hours }}</strong></div>
        <p class="support-contact-hint">添加微信后，发送软件账号和所需名额。</p>
      </div>
    </div>
    <template #footer>
      <el-button @click="emit('update:modelValue', false)">关闭</el-button>
      <el-button v-if="live && account" type="primary" :icon="Refresh" :loading="refreshing" @click="emit('refresh')">刷新授权</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.support-contact-content { display: flex; align-items: center; gap: 28px; }
.support-contact-qr { width: 260px; flex: 0 0 260px; padding: 6px; background: #fff; border: 1px solid #e5e7eb; border-radius: 12px; }
.support-contact-qr img { display: block; width: 100%; height: auto; border-radius: 8px; }
.support-contact-details { flex: 1; min-width: 0; }
.support-contact-details h3 { margin: 0 0 24px; color: #1f2937; font-size: 19px; font-weight: 600; }
.support-contact-field { display: flex; flex-direction: column; align-items: flex-start; gap: 7px; margin-bottom: 20px; }
.support-contact-field > span { font-size: 12px; color: #64748b; }
.support-contact-field strong { font-size: 14px; font-weight: 500; color: #1f2937; overflow-wrap: anywhere; user-select: text; }
.support-contact-hint { margin: 4px 0 0; color: #64748b; font-size: 12px; line-height: 1.7; }
.support-contact-error { margin-bottom: 16px; padding: 8px 12px; background: #fff7ed; color: #9a3412; font-size: 12px; border-radius: 6px; }
@media (max-width: 620px) { .support-contact-content { flex-direction: column; gap: 20px; } .support-contact-qr { width: 250px; flex-basis: auto; } .support-contact-details { width: 100%; } }
</style>
