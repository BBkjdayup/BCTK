<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import type { Question, QuestionType } from '../types/domain'
import { effectiveQuestionTypes } from '../utils/questionTypes'

type StatisticsSection = 'overview' | 'types' | 'subjects' | 'tags'
type Period = 'settings' | '30' | '90' | 'all'
type RecentMetric = 'added' | 'used'

interface DistributionItem {
  id: string
  label: string
  value: number
  color: string
}

interface ChapterStatistics {
  id: string
  name: string
  subject: string
  count: number
}

interface TagStatistics {
  id: string
  name: string
  count: number
}

const PAGE_SIZE = 100
const MAX_PAGES = 10_000
const DISTRIBUTION_COLORS = ['#2f68e8', '#18a6a3', '#f59e0b', '#7c3aed', '#e85d75', '#0f9f62']
const appStore = useAppStore()
const activeSection = ref<StatisticsSection>('overview')
const period = ref<Period>('settings')
const recentAddedDays = ref(30)
const recentUsedDays = ref(30)
const questions = ref<Question[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)
let loadSequence = 0
let disposed = false

function isCurrentLoad(sequence: number) {
  return !disposed && sequence === loadSequence
}

async function loadQuestions() {
  const sequence = ++loadSequence
  loading.value = true
  loadError.value = null

  try {
    const loaded: Question[] = []
    const seenIds = new Set<string>()
    let expectedTotal: number | null = null

    for (let page = 1; page <= MAX_PAGES; page += 1) {
      const result = await backend.listQuestions({
        keyword: '',
        tagIds: [],
        tagMatchMode: 'all',
        usage: 'all',
        deleted: false,
        page,
        pageSize: PAGE_SIZE,
      })

      if (!isCurrentLoad(sequence)) return
      if (!Number.isSafeInteger(result.total) || result.total < 0) {
        throw new Error('题库返回了无效的题目总数。')
      }
      if (result.items.length > PAGE_SIZE) {
        throw new Error('题库单页返回数量超过请求上限。')
      }
      if (expectedTotal === null) expectedTotal = result.total
      if (result.total !== expectedTotal) {
        throw new Error('统计期间题库内容发生了变化，请重新加载。')
      }

      for (const question of result.items) {
        if (seenIds.has(question.id)) {
          throw new Error('题库分页返回了重复题目，请重新加载。')
        }
        seenIds.add(question.id)
        loaded.push(question)
      }

      if (loaded.length >= expectedTotal) {
        if (loaded.length !== expectedTotal) {
          throw new Error('题目数量与题库总数不一致，请重新加载。')
        }
        if (isCurrentLoad(sequence)) questions.value = loaded
        return
      }

      if (result.items.length === 0) {
        throw new Error('题库分页提前结束，尚未取得全部题目。')
      }
    }

    throw new Error(`题目分页超过 ${MAX_PAGES.toLocaleString('zh-CN')} 页，已停止加载以避免无限请求。`)
  } catch (reason) {
    if (!isCurrentLoad(sequence)) return
    questions.value = []
    loadError.value = errorMessage(reason, '统计数据加载失败。')
  } finally {
    if (isCurrentLoad(sequence)) loading.value = false
  }
}

onMounted(() => {
  void loadQuestions()
  void backend.getSettings()
    .then((settings) => {
      recentAddedDays.value = settings.recentAddedDays
      recentUsedDays.value = settings.recentUsedDays
    })
    .catch(() => undefined)
})

onBeforeUnmount(() => {
  disposed = true
  loadSequence += 1
})

const periodOptions = computed<Array<{ value: Period; label: string }>>(() => [
  { value: 'settings', label: `按设置（新增 ${recentAddedDays.value} 天 / 使用 ${recentUsedDays.value} 天）` },
  { value: '30', label: '最近 30 天' },
  { value: '90', label: '最近 90 天' },
  { value: 'all', label: '全部时间' },
])

function selectedDays(metric: RecentMetric): number | null {
  if (period.value === 'all') return null
  if (period.value === 'settings') {
    return metric === 'added' ? recentAddedDays.value : recentUsedDays.value
  }
  return Number(period.value)
}

function metricPeriodLabel(metric: RecentMetric) {
  const days = selectedDays(metric)
  return days === null ? '全部时间' : `最近 ${days} 天`
}

function isInSelectedPeriod(timestamp: number | null | undefined, metric: RecentMetric) {
  if (timestamp == null || !Number.isFinite(timestamp)) return false
  const days = selectedDays(metric)
  return days === null || timestamp >= Date.now() - days * 24 * 60 * 60 * 1000
}

const typeData = computed<DistributionItem[]>(() => {
  const definitions = effectiveQuestionTypes(appStore.questionTypes)
  const counts = new Map<QuestionType, number>(definitions.map((definition) => [definition.code, 0]))
  for (const question of questions.value) counts.set(question.type, (counts.get(question.type) ?? 0) + 1)
  return definitions.map((definition, index) => ({
    id: definition.code,
    label: definition.name,
    value: counts.get(definition.code) ?? 0,
    color: DISTRIBUTION_COLORS[index % DISTRIBUTION_COLORS.length],
  }))
})

const subjectData = computed<DistributionItem[]>(() => {
  const subjects = [...appStore.subjects].sort((a, b) => a.sortOrder - b.sortOrder)
  const subjectIds = new Set(subjects.map((subject) => subject.id))
  const counts = new Map(subjects.map((subject) => [subject.id, 0]))
  let uncategorizedCount = 0

  for (const question of questions.value) {
    if (subjectIds.has(question.subjectId)) counts.set(question.subjectId, (counts.get(question.subjectId) ?? 0) + 1)
    else uncategorizedCount += 1
  }

  const data = subjects
    .map((subject, index) => ({
      id: subject.id,
      label: subject.name,
      value: counts.get(subject.id) ?? 0,
      color: DISTRIBUTION_COLORS[index % DISTRIBUTION_COLORS.length],
    }))
    .filter((item) => item.value > 0)

  if (uncategorizedCount > 0) {
    data.push({
      id: '__uncategorized__',
      label: '未分类学科',
      value: uncategorizedCount,
      color: '#94a3b8',
    })
  }
  return data
})

const chapterData = computed<ChapterStatistics[]>(() => {
  const chapterLookup = new Map<string, { name: string; subject: string; sortOrder: number }>()
  for (const subject of appStore.subjects) {
    for (const chapter of subject.chapters) {
      chapterLookup.set(chapter.id, {
        name: chapter.name,
        subject: subject.name,
        sortOrder: chapter.sortOrder,
      })
    }
  }

  const counts = new Map<string, number>()
  let uncategorizedCount = 0
  for (const question of questions.value) {
    if (chapterLookup.has(question.chapterId)) counts.set(question.chapterId, (counts.get(question.chapterId) ?? 0) + 1)
    else uncategorizedCount += 1
  }

  const data = [...counts.entries()].map(([id, count]) => {
    const chapter = chapterLookup.get(id)!
    return { id, name: chapter.name, subject: chapter.subject, count, sortOrder: chapter.sortOrder }
  })
  if (uncategorizedCount > 0) {
    data.push({ id: '__uncategorized__', name: '未分类章节', subject: '未分类学科', count: uncategorizedCount, sortOrder: Number.MAX_SAFE_INTEGER })
  }

  return data
    .sort((a, b) => b.count - a.count || a.sortOrder - b.sortOrder || a.name.localeCompare(b.name, 'zh-CN'))
    .map(({ id, name, subject, count }) => ({ id, name, subject, count }))
})

const tagData = computed<TagStatistics[]>(() => {
  const tags = [...appStore.tags]
  const knownTagIds = new Set(tags.map((tag) => tag.id))
  const counts = new Map(tags.map((tag) => [tag.id, 0]))
  const missingTags = new Map<string, string>()

  for (const question of questions.value) {
    const uniqueTags = new Map(question.tags.map((tag) => [tag.id, tag]))
    for (const tag of uniqueTags.values()) {
      if (knownTagIds.has(tag.id)) counts.set(tag.id, (counts.get(tag.id) ?? 0) + 1)
      else missingTags.set(tag.id, tag.name)
    }
  }

  const data = tags.map((tag) => ({ id: tag.id, name: tag.name, count: counts.get(tag.id) ?? 0 }))
  for (const [id, name] of missingTags) {
    const count = questions.value.filter((question) => question.tags.some((tag) => tag.id === id)).length
    data.push({ id, name, count })
  }
  return data.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name, 'zh-CN'))
})

const totalQuestions = computed(() => questions.value.length)
const newlyAddedCount = computed(() => questions.value.filter((question) => isInSelectedPeriod(question.createdAt, 'added')).length)
const usedInPeriodCount = computed(() => questions.value.filter((question) => isInSelectedPeriod(question.lastUsedAt, 'used')).length)
const neverUsedCount = computed(() => questions.value.filter((question) => question.lastUsedAt == null).length)
const neverUsedPercent = computed(() => totalQuestions.value === 0 ? 0 : (neverUsedCount.value / totalQuestions.value) * 100)
const maxTypeValue = computed(() => Math.max(0, ...typeData.value.map((item) => item.value)))
const maxChapterCount = computed(() => Math.max(0, ...chapterData.value.map((item) => item.count)))

function relativeHeight(value: number, maximum: number) {
  return maximum <= 0 ? 0 : (value / maximum) * 100
}

function percentage(value: number, total: number) {
  return total <= 0 ? '0.0' : ((value / total) * 100).toFixed(1)
}

const donutStyle = computed(() => {
  if (totalQuestions.value === 0 || subjectData.value.length === 0) return { background: '#e2e8f0' }
  let cursor = 0
  const stops = subjectData.value.map((item) => {
    const start = cursor
    cursor += (item.value / totalQuestions.value) * 100
    return `${item.color} ${start}% ${cursor}%`
  })
  return { background: `conic-gradient(${stops.join(', ')})` }
})

const metrics = computed(() => [
  { label: '题目总数', value: totalQuestions.value, note: '当前全部未删除题目', tone: 'primary' },
  { label: '期间新增', value: newlyAddedCount.value, note: metricPeriodLabel('added'), tone: 'dark' },
  { label: '期间使用', value: usedInPeriodCount.value, note: `${metricPeriodLabel('used')}内最近使用过`, tone: 'dark' },
  { label: '从未使用', value: neverUsedCount.value, note: `占题库 ${neverUsedPercent.value.toFixed(1)}%`, tone: 'warning' },
])

</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar statistics-sidebar">
      <div class="sidebar-title">题库统计</div>
      <nav class="sidebar-menu" aria-label="统计页面导航">
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'overview' }" :aria-current="activeSection === 'overview' ? 'page' : undefined" @click="activeSection = 'overview'">
          <span class="nav-icon" aria-hidden="true" />总览
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'types' }" :aria-current="activeSection === 'types' ? 'page' : undefined" @click="activeSection = 'types'">
          <span class="nav-icon" aria-hidden="true" />题型分析
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'subjects' }" :aria-current="activeSection === 'subjects' ? 'page' : undefined" @click="activeSection = 'subjects'">
          <span class="nav-icon" aria-hidden="true" />学科章节
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'tags' }" :aria-current="activeSection === 'tags' ? 'page' : undefined" @click="activeSection = 'tags'">
          <span class="nav-icon" aria-hidden="true" />标签使用
        </button>
      </nav>
    </aside>

    <section class="page-main statistics-page" :aria-busy="loading">
      <div class="statistics-toolbar">
        <div class="period-switch" role="group" aria-label="统计时间范围">
          <button
            v-for="option in periodOptions"
            :key="option.value"
            type="button"
            :class="{ 'is-active': period === option.value }"
            :aria-pressed="period === option.value"
            :aria-label="`显示${option.label}的新增和使用统计`"
            @click="period = option.value"
          >
            {{ option.label }}
          </button>
        </div>
      </div>

      <div v-if="loading" class="surface state-card" role="status" aria-live="polite">
        <el-skeleton :rows="7" animated />
        <span class="sr-only">正在加载全部题目并生成统计数据</span>
      </div>

      <div v-else-if="loadError" class="surface state-card state-card--error" role="alert">
        <strong>统计数据加载失败</strong>
        <p>{{ loadError }}</p>
        <el-button type="primary" aria-label="重新加载统计数据" @click="loadQuestions">重新加载</el-button>
      </div>

      <template v-else>
        <div class="metric-grid" aria-label="题库统计摘要">
          <article v-for="item in metrics" :key="item.label" class="surface metric-card">
            <span>{{ item.label }}</span>
            <strong :class="`is-${item.tone}`">{{ item.value }}</strong>
            <small :class="`is-${item.tone}`">{{ item.note }}</small>
          </article>
        </div>

        <div v-if="totalQuestions === 0" class="surface state-card empty-card" role="status">
          <strong>题库中还没有题目</strong>
          <p>录入或导入题目后，这里会自动显示真实的题型、学科、章节和标签统计。</p>
        </div>

        <div v-else-if="activeSection === 'overview'" class="overview-grid">
          <article class="surface chart-card type-chart-card" role="img" :aria-label="`题型数量分布，共 ${totalQuestions} 道题`">
            <header><h2>各题型数量</h2><p>当前全部未删除题目</p></header>
            <div class="vertical-bars" aria-hidden="true">
              <div v-for="item in typeData" :key="item.id" class="vertical-bar">
                <strong>{{ item.value }}</strong>
                <div class="bar-track"><i :style="{ height: `${relativeHeight(item.value, maxTypeValue)}%`, background: item.color }" /></div>
                <span>{{ item.label.replace('题', '') }}</span>
              </div>
            </div>
          </article>

          <article class="surface chart-card subject-chart-card" role="img" :aria-label="`学科分布，共 ${totalQuestions} 道题`">
            <header><h2>学科分布</h2><p>按题目数量统计</p></header>
            <div class="donut-wrap" aria-hidden="true">
              <div class="donut" :style="donutStyle"><div><strong>{{ totalQuestions }}</strong><span>题目总数</span></div></div>
              <div class="legend">
                <span v-for="item in subjectData" :key="item.id"><i :style="{ background: item.color }" />{{ item.label }}（{{ item.value }}）</span>
              </div>
            </div>
          </article>

          <article class="surface chart-card chapter-card">
            <header><h2>章节题量排行</h2><p>当前全部学科</p></header>
            <div class="chapter-list">
              <div v-for="item in chapterData.slice(0, 4)" :key="item.id">
                <p><span>{{ item.name }}</span><strong>{{ item.count }}</strong></p>
                <el-progress :percentage="relativeHeight(item.count, maxChapterCount)" :show-text="false" :stroke-width="6" />
              </div>
            </div>
          </article>
        </div>

        <div v-else-if="activeSection === 'types'" class="surface detail-card">
          <header class="detail-card__header">
            <div><h2>题型构成</h2><p>共 {{ totalQuestions }} 道题</p></div>
            <el-tag effect="plain">{{ typeData.filter((item) => item.value > 0).length }} 种已使用题型</el-tag>
          </header>
          <div class="horizontal-bars" aria-label="题型数量条形图">
            <div v-for="item in typeData" :key="item.id" class="horizontal-bar">
              <span>{{ item.label }}</span>
              <div><i :style="{ width: `${relativeHeight(item.value, maxTypeValue)}%`, background: item.color }" /></div>
              <strong>{{ item.value }} 题</strong>
            </div>
          </div>
          <el-table :data="typeData" class="statistics-table" aria-label="题型统计表">
            <el-table-column prop="label" label="题型" />
            <el-table-column label="题目数量"><template #default="{ row }">{{ row.value }} 题</template></el-table-column>
            <el-table-column label="题库占比"><template #default="{ row }">{{ percentage(row.value, totalQuestions) }}%</template></el-table-column>
          </el-table>
        </div>

        <div v-else-if="activeSection === 'subjects'" class="subject-detail-grid">
          <article class="surface detail-card subject-detail">
            <header class="detail-card__header"><div><h2>学科题量</h2><p>题目按所属学科统计</p></div></header>
            <div class="donut-wrap donut-wrap--large">
              <div class="donut donut--large" :style="donutStyle" role="img" :aria-label="`学科分布，共 ${totalQuestions} 道题`"><div><strong>{{ totalQuestions }}</strong><span>总题量</span></div></div>
              <div class="subject-legend">
                <div v-for="item in subjectData" :key="item.id">
                  <span><i :style="{ background: item.color }" />{{ item.label }}</span>
                  <strong>{{ item.value }} 题</strong>
                </div>
              </div>
            </div>
          </article>
          <article class="surface detail-card">
            <header class="detail-card__header"><div><h2>章节排行</h2><p>按题目数量从高到低</p></div></header>
            <div class="rank-list">
              <div v-for="(item, index) in chapterData" :key="item.id">
                <span class="rank">{{ index + 1 }}</span>
                <div><strong>{{ item.name }}</strong><small>{{ item.subject }}</small></div>
                <b>{{ item.count }} 题</b>
              </div>
            </div>
          </article>
        </div>

        <div v-else class="surface detail-card">
          <header class="detail-card__header">
            <div><h2>标签使用排行</h2><p>按当前关联题目数从高到低</p></div>
            <span class="muted">共 {{ tagData.length }} 个标签</span>
          </header>
          <div v-if="tagData.length" class="tag-grid">
            <article v-for="tag in tagData" :key="tag.id" class="tag-stat">
              <span>{{ tag.name }}</span>
              <strong>{{ tag.count }}</strong>
              <small>{{ tag.count > 0 ? `占题库 ${percentage(tag.count, totalQuestions)}%` : '尚未用于任何题目' }}</small>
            </article>
          </div>
          <div v-else class="inline-empty" role="status">当前还没有标签。</div>
          <div class="info-banner">标签统计只展示当前真实关联情况，不会自动合并或删除已有标签。</div>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.statistics-sidebar {
  padding-top: 2px;
}

.nav-icon {
  width: 18px;
  height: 18px;
  border: 1.5px solid #94a3b8;
  border-radius: 5px;
}

.sidebar-menu__item.is-active .nav-icon {
  border-color: #3b82f6;
}

.statistics-page {
  display: flex;
  flex-direction: column;
}

.statistics-toolbar {
  margin-bottom: 14px;
  display: flex;
  justify-content: flex-end;
}

.period-switch {
  display: flex;
  overflow: hidden;
  border: 1px solid #d8e0ea;
  border-radius: 8px;
  background: #fff;
}

.period-switch button {
  min-height: 34px;
  padding: 0 12px;
  border: 0;
  border-right: 1px solid #e2e8f0;
  background: transparent;
  color: #64748b;
  cursor: pointer;
  font-size: 11px;
}

.period-switch button:last-child {
  border-right: 0;
}

.period-switch button:hover,
.period-switch button:focus-visible {
  background: #eff6ff;
  color: #2563eb;
}

.period-switch button.is-active {
  background: #2563eb;
  color: #fff;
}

.metric-grid {
  margin-bottom: 14px;
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 13px;
}

.metric-card {
  min-height: 110px;
  padding: 18px;
  display: flex;
  flex-direction: column;
}

.metric-card > span {
  color: #64748b;
  font-size: 11px;
}

.metric-card strong {
  margin-top: 8px;
  color: #111827;
  font-size: 27px;
  line-height: 1;
}

.metric-card small {
  margin-top: 7px;
  color: #475569;
  font-size: 10px;
}

.metric-card .is-primary { color: #2563eb; }
.metric-card small.is-warning { color: #d97706; }

.state-card {
  min-height: 320px;
  padding: 30px;
}

.state-card--error,
.empty-card {
  display: grid;
  place-items: center;
  align-content: center;
  text-align: center;
}

.state-card--error strong,
.empty-card strong {
  color: #1e293b;
  font-size: 17px;
}

.state-card--error p,
.empty-card p {
  max-width: 560px;
  margin: 8px 0 18px;
  color: #64748b;
  font-size: 12px;
}

.overview-grid {
  min-height: 430px;
  flex: 1;
  display: grid;
  grid-template-columns: 1.3fr 1fr .85fr;
  gap: 13px;
}

.chart-card,
.detail-card {
  padding: 18px;
}

.chart-card header h2,
.detail-card__header h2 {
  margin: 0;
  color: #111827;
  font-size: 15px;
}

.chart-card header p,
.detail-card__header p {
  margin: 4px 0 0;
  color: #64748b;
  font-size: 10px;
}

.vertical-bars {
  height: 290px;
  margin-top: 24px;
  padding: 10px 18px 0;
  display: flex;
  align-items: flex-end;
  justify-content: space-around;
  border-bottom: 1px solid #dfe5ed;
}

.vertical-bar {
  width: 13%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.vertical-bar strong {
  margin-bottom: 5px;
  color: #64748b;
  font-size: 9px;
  opacity: 0;
  transition: opacity .15s ease;
}

.vertical-bar:hover strong {
  opacity: 1;
}

.bar-track {
  min-height: 0;
  flex: 1;
  width: 34px;
  display: flex;
  align-items: flex-end;
}

.bar-track i {
  width: 100%;
  min-height: 2px;
  border-radius: 5px 5px 0 0;
}

.vertical-bar span {
  padding: 10px 0 0;
  color: #475569;
  font-size: 10px;
  white-space: nowrap;
}

.donut-wrap {
  min-height: 330px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.donut {
  width: 170px;
  height: 170px;
  padding: 34px;
  border-radius: 50%;
}

.donut > div {
  width: 100%;
  height: 100%;
  display: grid;
  place-items: center;
  align-content: center;
  border-radius: 50%;
  background: #fff;
}

.donut strong {
  font-size: 24px;
}

.donut span {
  margin-top: 3px;
  color: #64748b;
  font-size: 10px;
}

.legend {
  margin-top: 28px;
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 12px;
  color: #475569;
  font-size: 9px;
}

.legend i,
.subject-legend i {
  width: 7px;
  height: 7px;
  margin-right: 4px;
  display: inline-block;
  border-radius: 50%;
}

.chapter-list {
  margin-top: 20px;
}

.chapter-list > div {
  margin-bottom: 17px;
}

.chapter-list p {
  margin: 0 0 7px;
  display: flex;
  justify-content: space-between;
  gap: 10px;
  color: #334155;
  font-size: 10px;
}

.chapter-list p span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-card {
  min-height: 460px;
}

.detail-card__header {
  margin-bottom: 22px;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
}

.horizontal-bars {
  margin-bottom: 25px;
  display: grid;
  gap: 14px;
}

.horizontal-bar {
  display: grid;
  grid-template-columns: 110px minmax(200px, 1fr) 70px;
  align-items: center;
  gap: 13px;
  color: #334155;
  font-size: 11px;
}

.horizontal-bar > div {
  height: 10px;
  overflow: hidden;
  border-radius: 6px;
  background: #eef2f7;
}

.horizontal-bar i {
  height: 100%;
  display: block;
  border-radius: 6px;
}

.horizontal-bar strong {
  text-align: right;
}

.statistics-table {
  border-top: 1px solid #edf1f5;
}

.subject-detail-grid {
  display: grid;
  grid-template-columns: 1fr 1.2fr;
  gap: 13px;
}

.donut-wrap--large {
  min-height: 0;
  flex-direction: row;
  gap: 42px;
}

.donut--large {
  width: 200px;
  height: 200px;
  flex: 0 0 200px;
}

.subject-legend {
  min-width: 190px;
}

.subject-legend > div {
  padding: 10px 0;
  display: flex;
  justify-content: space-between;
  gap: 16px;
  border-bottom: 1px solid #edf1f5;
  color: #475569;
  font-size: 11px;
}

.rank-list > div {
  padding: 12px 0;
  display: grid;
  grid-template-columns: 28px 1fr auto;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #edf1f5;
}

.rank {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: 6px;
  background: #eff6ff;
  color: #2563eb;
  font-size: 10px;
  font-weight: 700;
}

.rank-list strong,
.rank-list small {
  display: block;
}

.rank-list strong {
  font-size: 11px;
}

.rank-list small {
  margin-top: 3px;
  color: #94a3b8;
  font-size: 9px;
}

.rank-list b {
  font-size: 11px;
}

.tag-grid {
  margin-bottom: 18px;
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
}

.tag-stat {
  padding: 18px;
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 8px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: #fbfdff;
  color: #334155;
}

.tag-stat span {
  font-size: 12px;
  font-weight: 600;
}

.tag-stat strong {
  color: #2563eb;
  font-size: 18px;
}

.tag-stat small {
  grid-column: 1 / -1;
  color: #64748b;
  font-size: 9px;
}

.inline-empty {
  padding: 36px;
  color: #64748b;
  text-align: center;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

@media (max-width: 1280px) {
  .overview-grid {
    grid-template-columns: 1.15fr .9fr .8fr;
  }

  .metric-card {
    padding: 15px;
  }

  .tag-grid {
    grid-template-columns: repeat(3, 1fr);
  }
}
</style>
