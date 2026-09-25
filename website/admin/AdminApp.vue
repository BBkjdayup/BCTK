<script setup>
import { computed, nextTick, onMounted, onUnmounted, reactive, ref } from 'vue'
import { createAdminClient } from './api.js'

const client = createAdminClient()
const logoUrl = import.meta.env.BASE_URL + 'tk-logo.png'
const session = ref(null)
const starting = ref(true)
const busy = ref(false)
const loading = ref(false)
const error = ref('')
const notice = ref('')
const view = ref('users')
const items = ref([])
const plans = ref([])
const support = ref(null)
const imageReading = ref(false)
const page = ref(1)
const hasMore = ref(false)
const search = ref('')
const status = ref('all')
const auditTarget = ref('')
const auditName = ref('')
const login = reactive({ account: '', password: '', code: '' })
const modal = ref(null)
const dialog = ref(null)
const draft = reactive({})
const formError = ref('')
const verificationCode = ref('')
const requireReauth = ref(false)
let loadSequence = 0
let lastActivity = Date.now()
let activityTimer
const activity = () => { lastActivity = Date.now() }

const titles = { users: '用户管理', plans: '套餐设置', support: '客服信息', audit: '变更记录' }
const statuses = { free: '免费使用', active: '付费有效', paused: '付费已暂停', expired: '付费已到期', scheduled: '付费未生效', disabled: '账号停用' }
const actions = { grant_create: '开通授权', grant_update: '修改授权', plan_create: '新增套餐', plan_update: '修改套餐',
  support_update: '修改客服信息',
  admin_login: '管理员登录', admin_logout: '退出登录', admin_reauth: '身份复核', admin_role_set: '调整管理权限' }
const dateText = value => value ? value.replaceAll('-', '/') : '—'
const timeText = value => new Intl.DateTimeFormat('zh-CN', { timeZone: 'Asia/Shanghai', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(new Date(value))
const today = () => new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Shanghai' }).format(new Date())
const planOptions = computed(() => plans.value.filter(p => p.enabled || p.id === modal.value?.original?.plan_id))

function clearSession() {
  session.value = null; client.clear(); items.value = []; plans.value = []; loadSequence++
  support.value = null; imageReading.value = false
  modal.value = null; dialog.value?.close()
}
function handleError(err, inForm = false) {
  if (err.status === 401) { clearSession(); error.value = '登录已失效，请重新登录'; return }
  if (err.code === 'REAUTH_REQUIRED') { requireReauth.value = true; formError.value = '请输入动态验证码后重新保存'; return }
  if (inForm) formError.value = err.message
  else error.value = err.message
}
async function load() {
  const sequence = ++loadSequence
  loading.value = true; error.value = ''; items.value = []
  try {
    if (view.value === 'support') {
      support.value = null
      const result = await client.request('/support-contact')
      if (sequence !== loadSequence) return
      support.value = result; hasMore.value = false
    } else if (view.value === 'plans') {
      const result = await client.request('/plans')
      if (sequence !== loadSequence) return
      plans.value = result.items; items.value = result.items; hasMore.value = false
    } else {
      const query = new URLSearchParams({ page: String(page.value) })
      if (view.value === 'users') { query.set('q', search.value.trim()); query.set('status', status.value) }
      if (view.value === 'audit' && auditTarget.value) query.set('target', auditTarget.value)
      const result = await client.request(`/${view.value}?${query}`)
      if (sequence !== loadSequence) return
      items.value = result.items; hasMore.value = result.has_more
    }
  } catch (err) { if (sequence === loadSequence) handleError(err) }
  finally { if (sequence === loadSequence) loading.value = false }
}
async function navigate(next) {
  view.value = next; page.value = 1; notice.value = ''; auditTarget.value = ''; auditName.value = ''
  await load()
}
async function signIn() {
  busy.value = true; error.value = ''
  try { session.value = await client.request('/session/login', 'POST', { ...login }); lastActivity = Date.now(); await load() }
  catch (err) { error.value = err.status === 401 ? '账号、密码或动态验证码无效；已用过的验证码需等待更新' : err.message }
  finally { login.password = ''; login.code = ''; busy.value = false }
}
async function signOut() {
  busy.value = true
  try { await client.request('/session/logout', 'POST'); clearSession(); error.value = ''; notice.value = '' }
  catch (err) { handleError(err) }
  finally { busy.value = false }
}
async function showModal(value) {
  modal.value = value; formError.value = ''; verificationCode.value = ''; requireReauth.value = false
  await nextTick(); dialog.value.showModal()
}
function closeModal() { if (busy.value) return; dialog.value?.close(); modal.value = null; formError.value = ''; imageReading.value = false }
function onCancel(event) { if (busy.value) event.preventDefault(); else closeModal() }
async function editGrant(user) {
  try {
    plans.value = (await client.request('/plans')).items
    Object.assign(draft, { plan_id: user.plan_id || '', seat_limit: user.seat_limit || 5,
      starts_on: user.starts_on || today(), ends_on: user.ends_on || '', paused: !!user.paused,
      expected_version: user.version, reason: '' })
    await showModal({ kind: 'grant', step: 'edit', original: { ...user } })
  } catch (err) { handleError(err) }
}
function selectPlan() {
  const chosen = plans.value.find(p => p.id === draft.plan_id)
  if (chosen?.kind === 'fixed') draft.seat_limit = chosen.seats
}
async function editPlan(plan) {
  Object.assign(draft, { name: plan?.name || '', kind: plan?.kind || 'fixed', seats: plan?.seats ?? 5,
    enabled: plan?.enabled ?? true, expected_version: plan?.version || 0, reason: '' })
  await showModal({ kind: 'plan', step: 'edit', original: plan ? { ...plan } : null })
}
function safeQr(value) {
  return typeof value === 'string' && value.length <= 1398128 && /^data:image\/(?:jpeg|png);base64,[A-Za-z0-9+/]+={0,2}$/.test(value) ? value : ''
}
async function editSupport() {
  Object.assign(draft, { ...support.value, expected_version: support.value.version, reason: '' })
  await showModal({ kind: 'support', step: 'edit', original: { ...support.value } })
}
function selectSupportQr(event) {
  const file = event.target.files?.[0]
  event.target.value = ''
  if (!file) return
  if (!['image/png', 'image/jpeg'].includes(file.type) || file.size > 1024 * 1024) { formError.value = '请选择不超过 1 MB 的 JPG 或 PNG 图片'; return }
  const editing = modal.value
  const reader = new FileReader()
  imageReading.value = true; formError.value = ''
  reader.onload = () => {
    if (modal.value !== editing) return
    imageReading.value = false
    if (!safeQr(reader.result)) { formError.value = '二维码图片格式无效'; return }
    draft.qr_code = reader.result
  }
  reader.onerror = () => { if (modal.value === editing) { imageReading.value = false; formError.value = '图片读取失败，请重新选择' } }
  reader.readAsDataURL(file)
}
function review() {
  formError.value = ''
  if (imageReading.value) { formError.value = '请等待图片读取完成'; return }
  if (!draft.reason.trim() || draft.reason.trim().length > 300) { formError.value = '请填写调整原因，最多 300 字'; return }
  if (modal.value.kind === 'grant') {
    if (!draft.plan_id || !Number.isInteger(draft.seat_limit) || draft.seat_limit < 1 || draft.seat_limit > 10000) { formError.value = '请选择套餐并填写 1 至 10000 的付费名额'; return }
    if (!draft.starts_on || !draft.ends_on || draft.ends_on < draft.starts_on) { formError.value = '请检查开始日期和到期日期'; return }
  } else if (modal.value.kind === 'support') {
    if (!draft.display_name.trim() || draft.display_name.trim().length > 40 || !/^[A-Za-z][A-Za-z0-9_-]{5,19}$/.test(draft.wechat_id.trim())
      || draft.service_hours.trim().length > 80 || (draft.qr_code && !safeQr(draft.qr_code))) { formError.value = '请检查客服名称、微信号和二维码'; return }
  } else if (!draft.name.trim() || (draft.kind === 'fixed' && (!Number.isInteger(draft.seats) || draft.seats < 1 || draft.seats > 10000))) { formError.value = '请检查套餐名称和人数'; return }
  if (diffRows.value.every(r => r.before === r.after)) { formError.value = '没有需要保存的变更'; return }
  requireReauth.value = Date.now() >= new Date(session.value.write_authorized_until).getTime()
  modal.value.step = 'review'
}
const diffRows = computed(() => {
  if (!modal.value || modal.value.kind === 'audit') return []
  const before = modal.value.original
  const row = (label, old, value) => ({ label, before: old ?? '—', after: value ?? '—' })
  if (modal.value.kind === 'support') {
    return [row('客服名称', before.display_name, draft.display_name.trim()), row('微信号', before.wechat_id, draft.wechat_id.trim()),
      row('联系时间', before.service_hours || '未设置', draft.service_hours.trim() || '未设置'),
      row('添加好友二维码', before.qr_code ? '已设置' : '未设置', draft.qr_code === before.qr_code ? (before.qr_code ? '已设置' : '未设置') : draft.qr_code ? '已更换' : '未设置')]
  }
  if (modal.value.kind === 'grant') {
    const planName = draft.plan_id === before.plan_id ? before.plan_name : plans.value.find(p => p.id === draft.plan_id)?.name
    return [row('套餐', before.plan_name, planName), row('付费名额', before.seat_limit, draft.seat_limit), row('付费生效后总名额', before.seat_limit == null ? 1 : before.seat_limit + 1, draft.seat_limit + 1),
      row('开始日期', dateText(before.starts_on), dateText(draft.starts_on)), row('到期日期', dateText(before.ends_on), dateText(draft.ends_on)),
      row('访问设置', before.version ? (before.paused ? '暂停' : '正常') : '未开通', draft.paused ? '暂停' : '正常')]
  }
  return [row('套餐名称', before?.name, draft.name.trim()), row('类型', before ? (before.kind === 'fixed' ? '固定人数' : '自定义人数') : null, draft.kind === 'fixed' ? '固定人数' : '自定义人数'),
    row('人数', before?.kind === 'custom' ? '自定义' : before?.seats, draft.kind === 'custom' ? '自定义' : draft.seats),
    row('状态', before ? (before.enabled ? '可选' : '已停用') : null, draft.enabled ? '可选' : '已停用')]
})
async function save() {
  busy.value = true; formError.value = ''
  try {
    if (requireReauth.value) {
      const result = await client.request('/session/reauth', 'POST', { code: verificationCode.value })
      session.value.write_authorized_until = result.write_authorized_until; requireReauth.value = false; verificationCode.value = ''
    }
    if (modal.value.kind === 'support') {
      const { display_name, wechat_id, service_hours, qr_code, expected_version, reason } = draft
      await client.request('/support-contact', 'PUT', { display_name: display_name.trim(), wechat_id: wechat_id.trim(), service_hours: service_hours.trim(), qr_code, expected_version, reason: reason.trim() })
    } else if (modal.value.kind === 'grant') {
      const { plan_id, seat_limit, starts_on, ends_on, paused, expected_version, reason } = draft
      await client.request(`/users/${modal.value.original.id}/grant`, 'PUT', { plan_id, seat_limit, starts_on, ends_on, paused, expected_version, reason: reason.trim() })
    } else {
      const { name, kind, seats, enabled, expected_version, reason } = draft
      const id = modal.value.original?.id
      await client.request(`/plans${id ? `/${id}` : ''}`, id ? 'PUT' : 'POST', { name: name.trim(), kind, seats: kind === 'custom' ? null : seats, enabled, expected_version, reason: reason.trim() })
    }
    busy.value = false; closeModal(); notice.value = '已保存，变更记录已同步写入'; await load()
  } catch (err) { handleError(err, true) }
  finally { busy.value = false }
}
async function userAudit(user) {
  view.value = 'audit'; page.value = 1; auditTarget.value = user.id; auditName.value = user.username; notice.value = ''; await load()
}
const auditRows = computed(() => {
  const record = modal.value?.record
  if (!record) return []
  const labels = { name: '套餐名称', kind: '类型', seats: '付费名额', enabled: '启用', plan_name: '套餐', seat_limit: '付费名额', starts_on: '开始日期', ends_on: '到期日期', paused: '暂停付费名额', display_name: '客服名称', wechat_id: '微信号', service_hours: '联系时间', version: '版本' }
  const format = value => value == null ? '—' : typeof value === 'boolean' ? (value ? '是' : '否') : value === 'fixed' ? '固定人数' : value === 'custom' ? '自定义人数' : String(value)
  const rows = Object.entries(labels).filter(([key]) => key in (record.before_value || {}) || key in (record.after_value || {}))
    .map(([key, label]) => ({ label, before: format(record.before_value?.[key]), after: format(record.after_value?.[key]) }))
  if (record.action === 'support_update') rows.push({ label: '添加好友二维码', before: '原图片', after: record.before_value?.qr_fingerprint === record.after_value?.qr_fingerprint ? '原图片' : '已变更' })
  return rows
})
onMounted(async () => {
  window.addEventListener('pointerdown', activity, { passive: true })
  window.addEventListener('keydown', activity)
  activityTimer = window.setInterval(async () => {
    if (!session.value || busy.value) return
    if (Date.now() - lastActivity >= 15 * 60 * 1000 || Date.now() >= new Date(session.value.expires_at).getTime()) {
      // Hide sensitive data immediately; server expiry remains authoritative if offline.
      client.request('/session/logout', 'POST').catch(() => {})
      clearSession(); error.value = '登录已超时，请重新登录'
    }
  }, 15000)
  try { session.value = await client.request('/session'); await load() }
  catch (err) { if (err.status !== 401) error.value = err.message }
  finally { starting.value = false }
})
onUnmounted(() => {
  window.clearInterval(activityTimer)
  window.removeEventListener('pointerdown', activity)
  window.removeEventListener('keydown', activity)
})
</script>

<template>
  <div class="admin-shell">
    <header class="topbar">
      <a class="brand" href="https://tktiku.cn/" aria-label="TK 官网"><img :src="logoUrl" alt="TK"><span>管理控制台</span></a>
      <div v-if="session" class="account"><span>{{ session.username }}</span><button class="text-button" :disabled="busy" @click="signOut">退出</button></div>
    </header>
    <main v-if="starting" class="login-area" aria-live="polite">正在检查登录状态…</main>
    <main v-else-if="!session" class="login-area">
      <form class="login-form" @submit.prevent="signIn">
        <h1>管理员登录</h1>
        <label>账号<input v-model="login.account" name="username" autocomplete="username" maxlength="320" required></label>
        <label>密码<input v-model="login.password" name="password" type="password" autocomplete="current-password" maxlength="1024" required></label>
        <label>动态验证码<input v-model="login.code" name="otp" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]{6}" maxlength="6" placeholder="验证器中的 6 位数字" required></label>
        <p v-if="error" class="error-message" role="alert">{{ error }}</p>
        <button class="primary" :disabled="busy">{{ busy ? '正在验证…' : '登录' }}</button>
        <p class="muted small">仅限已开通管理权限的账号</p>
      </form>
    </main>
    <div v-else class="workspace">
      <nav class="sidebar" aria-label="管理功能">
        <p class="sidebar-label">小程序服务</p>
        <button v-for="(title, key) in titles" :key="key" :class="{ selected: view === key }" :aria-current="view === key ? 'page' : undefined" @click="navigate(key)">{{ title }}</button>
      </nav>
      <main class="content" :aria-busy="loading">
        <div class="page-heading"><h1>{{ titles[view] }}</h1><span v-if="view !== 'support'" class="muted small">{{ view === 'users' ? '题库拥有者' : view === 'plans' ? '套餐模板' : '操作留痕' }}</span></div>
        <p v-if="notice" class="notice" role="status">{{ notice }}</p>
        <p v-if="error" class="error-message" role="alert">{{ error }} <button class="text-button" @click="load">重试</button></p>
        <form v-if="view === 'users'" class="toolbar" @submit.prevent="page = 1; load()">
          <input v-model="search" type="search" aria-label="搜索用户" placeholder="用户名、邮箱或编号" maxlength="100">
          <select v-model="status" aria-label="授权状态" @change="page = 1; load()"><option value="all">全部状态</option><option v-for="(label, key) in statuses" :key="key" :value="key">{{ label }}</option></select>
          <button>查询</button>
        </form>
        <div v-else-if="view === 'plans'" class="toolbar"><span class="muted small">修改模板只影响后续开通的授权</span><button class="primary align-right" @click="editPlan(null)">新增套餐</button></div>
        <div v-else-if="view === 'audit'" class="toolbar"><span>{{ auditName ? `${auditName} 的记录` : '全部记录' }}</span><button v-if="auditTarget" class="text-button" @click="auditTarget = ''; auditName = ''; page = 1; load()">查看全部</button><button class="align-right" @click="load">刷新</button></div>
        <p v-if="loading" class="empty" role="status">正在加载…</p>
        <section v-else-if="view === 'support' && support" class="support-settings">
          <img v-if="safeQr(support.qr_code)" class="support-qr-preview" :src="safeQr(support.qr_code)" alt="客服添加好友二维码">
          <div class="support-settings-details">
            <dl class="details"><dt>客服名称</dt><dd>{{ support.display_name }}</dd><dt>微信号</dt><dd>{{ support.wechat_id }}</dd><dt>联系时间</dt><dd>{{ support.service_hours || '未设置' }}</dd></dl>
            <button class="primary" @click="editSupport">编辑客服信息</button>
          </div>
        </section>
        <p v-else-if="!items.length && !error" class="empty">{{ view === 'audit' ? '暂无变更记录' : '没有符合条件的记录' }}</p>
        <div v-else-if="items.length" class="table-wrap">
          <table v-if="view === 'users'" class="data-table users-table">
            <thead><tr><th>用户 / 账号</th><th>付费套餐</th><th>当前总名额</th><th>付费有效期</th><th>状态</th><th class="right">操作</th></tr></thead>
            <tbody><tr v-for="user in items" :key="user.id">
              <td data-label="用户"><strong>{{ user.username }}</strong><span class="secondary">{{ user.email || `ID ${user.id.slice(0, 8)}` }}</span></td>
              <td data-label="付费套餐">{{ user.plan_name || '未购买' }}<span class="secondary">永久免费 1 位{{ user.seat_limit ? ` + 付费 ${user.seat_limit} 位` : '' }}</span></td><td data-label="当前总名额" class="numeric">{{ user.total_seats ?? (user.seat_limit ? user.seat_limit + 1 : 1) }}</td>
              <td data-label="付费有效期" class="numeric">{{ dateText(user.ends_on) }}<span class="secondary">{{ user.starts_on ? `${dateText(user.starts_on)} 起` : '免费名额永久有效' }}</span></td>
              <td data-label="状态"><span class="status" :class="user.status">{{ statuses[user.status] }}</span></td>
              <td class="row-actions"><button class="text-button" :disabled="user.account_status !== 'active'" @click="editGrant(user)">{{ user.version ? '编辑授权' : '开通授权' }}</button><button class="text-button muted" @click="userAudit(user)">记录</button></td>
            </tr></tbody>
          </table>
          <table v-else-if="view === 'plans'" class="data-table">
            <thead><tr><th>套餐名称</th><th>类型</th><th>人数</th><th>状态</th><th class="right">操作</th></tr></thead>
            <tbody><tr v-for="plan in items" :key="plan.id"><td data-label="套餐">{{ plan.name }}</td><td data-label="类型">{{ plan.kind === 'fixed' ? '固定人数' : '自定义人数' }}</td><td data-label="人数">{{ plan.seats ?? '开通时填写' }}</td><td data-label="状态"><span class="status" :class="plan.enabled ? 'active' : 'paused'">{{ plan.enabled ? '可选' : '已停用' }}</span></td><td class="row-actions"><button class="text-button" @click="editPlan(plan)">编辑</button></td></tr></tbody>
          </table>
          <table v-else class="data-table">
            <thead><tr><th>时间 / 操作人</th><th>对象</th><th>操作</th><th>原因</th><th class="right">操作</th></tr></thead>
            <tbody><tr v-for="record in items" :key="record.id"><td data-label="时间 / 操作人" class="numeric">{{ timeText(record.created_at) }}<span class="secondary">{{ record.actor_name }}</span></td><td data-label="对象">{{ record.target_name }}</td><td data-label="操作">{{ actions[record.action] || record.action }}</td><td data-label="原因" class="reason-cell">{{ record.reason }}</td><td class="row-actions"><button class="text-button" @click="showModal({ kind: 'audit', record })">详情</button></td></tr></tbody>
          </table>
        </div>
        <div v-if="view === 'users' || view === 'audit'" class="pagination"><span class="muted small">第 {{ page }} 页</span><button :disabled="page === 1 || loading" @click="page--; load()">上一页</button><button :disabled="!hasMore || loading" @click="page++; load()">下一页</button></div>
      </main>
    </div>
    <dialog ref="dialog" aria-labelledby="dialog-title" @cancel="onCancel">
      <template v-if="modal">
        <div class="dialog-heading"><h2 id="dialog-title">{{ modal.kind === 'audit' ? '变更详情' : modal.step === 'review' ? '核对变更' : modal.kind === 'support' ? '编辑客服信息' : modal.kind === 'grant' ? '编辑授权' : modal.original ? '编辑套餐' : '新增套餐' }}</h2><button class="text-button" :disabled="busy" aria-label="关闭" @click="closeModal">关闭</button></div>
        <p v-if="modal.kind === 'grant'" class="dialog-subtitle">{{ modal.original.username }}<span class="secondary">{{ modal.original.email || modal.original.id }}</span></p>
        <template v-if="modal.kind === 'audit'">
          <dl class="details"><dt>操作</dt><dd>{{ actions[modal.record.action] || modal.record.action }}</dd><dt>操作人</dt><dd>{{ modal.record.actor_name }}</dd><dt>对象</dt><dd>{{ modal.record.target_name }}</dd><dt>时间</dt><dd>{{ timeText(modal.record.created_at) }}</dd></dl>
          <table v-if="auditRows.length" class="diff-table"><thead><tr><th>项目</th><th>修改前</th><th>修改后</th></tr></thead><tbody><tr v-for="row in auditRows" :key="row.label" :class="{ changed: row.before !== row.after }"><td>{{ row.label }}</td><td>{{ row.before }}</td><td>{{ row.after }}</td></tr></tbody></table>
          <p class="reason-text">{{ modal.record.reason }}</p>
          <div class="dialog-actions"><button @click="closeModal">关闭</button></div>
        </template>
        <form v-else-if="modal.step === 'edit'" @submit.prevent="review">
          <div v-if="modal.kind === 'grant'" class="form-grid">
            <label class="span-two">套餐<select v-model="draft.plan_id" required @change="selectPlan"><option value="" disabled>请选择套餐</option><option v-for="plan in planOptions" :key="plan.id" :value="plan.id">{{ plan.id === modal.original.plan_id && plan.name !== modal.original.plan_name ? `${modal.original.plan_name}（原授权）` : plan.name }}{{ plan.enabled ? '' : ' · 已停用' }}</option></select></label>
            <label>付费名额<input v-model.number="draft.seat_limit" type="number" min="1" max="10000" step="1" required></label>
            <label>付费名额状态<select v-model="draft.paused"><option :value="false">正常</option><option :value="true">暂停</option></select></label>
            <label>开始日期<input v-model="draft.starts_on" type="date" min="2020-01-01" max="2100-12-31" required></label>
            <label>到期日期<input v-model="draft.ends_on" type="date" :min="draft.starts_on || '2020-01-01'" max="2100-12-31" required></label>
          </div>
          <div v-else-if="modal.kind === 'support'" class="form-grid">
            <label>客服名称<input v-model="draft.display_name" name="display_name" maxlength="40" required></label>
            <label>微信号<input v-model="draft.wechat_id" name="wechat_id" maxlength="20" required></label>
            <label class="span-two">联系时间（选填）<input v-model="draft.service_hours" name="service_hours" maxlength="80"></label>
            <label class="span-two">添加好友二维码<input type="file" accept="image/jpeg,image/png" :disabled="imageReading" @change="selectSupportQr"><span class="muted small">JPG / PNG，最大 1 MB，宽高不超过 2048 像素</span></label>
            <div v-if="safeQr(draft.qr_code)" class="span-two support-image-edit"><img class="support-qr-preview" :src="safeQr(draft.qr_code)" alt="待保存的客服二维码"><button type="button" class="text-button" :disabled="imageReading" @click="draft.qr_code = ''">移除二维码</button></div>
          </div>
          <div v-else class="form-grid">
            <label class="span-two">套餐名称<input v-model="draft.name" maxlength="40" required></label>
            <label>类型<select v-model="draft.kind"><option value="fixed">固定人数</option><option value="custom">自定义人数</option></select></label>
            <label v-if="draft.kind === 'fixed'">人数<input v-model.number="draft.seats" type="number" min="1" max="10000" step="1" required></label>
            <label>状态<select v-model="draft.enabled"><option :value="true">可选</option><option :value="false">停用</option></select></label>
          </div>
          <label class="reason-field">调整原因<textarea v-model="draft.reason" rows="3" maxlength="300" required placeholder="填写开通依据或本次调整原因"></textarea></label>
          <p v-if="modal.kind === 'grant'" class="muted small">每个题库另有 1 个永久免费名额。5 人包增加 5 位，生效期间共 6 位；到期日包含当天（北京时间）。</p>
          <p v-if="formError" class="error-message" role="alert">{{ formError }}</p>
          <div class="dialog-actions"><button type="button" @click="closeModal">取消</button><button class="primary">核对变更</button></div>
        </form>
        <form v-else @submit.prevent="save">
          <table class="diff-table"><thead><tr><th>项目</th><th>修改前</th><th>修改后</th></tr></thead><tbody><tr v-for="row in diffRows" :key="row.label" :class="{ changed: row.before !== row.after }"><td>{{ row.label }}</td><td>{{ row.before }}</td><td>{{ row.after }}<span v-if="row.before === row.after" class="unchanged">未变</span></td></tr></tbody></table>
          <p class="reason-text">{{ draft.reason }}</p>
          <label v-if="requireReauth">动态验证码<input v-model="verificationCode" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]{6}" maxlength="6" required></label>
          <p v-if="formError" class="error-message" role="alert">{{ formError }}</p>
          <div class="dialog-actions"><button type="button" :disabled="busy" @click="modal.step = 'edit'; formError = ''">返回修改</button><button class="primary" :disabled="busy">{{ busy ? '正在保存…' : '确认保存' }}</button></div>
        </form>
      </template>
    </dialog>
  </div>
</template>
