<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import SubjectTree from '../components/SubjectTree.vue'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const bankStore = useQuestionBankStore()

async function selectTree(payload: { subjectId?: string; chapterId?: string }) {
  if (route.path === '/duplicate-check') {
    if (!payload.subjectId) {
      ElMessage.warning('题目查重需要选择一个学科或具体章节')
      return
    }
    bankStore.filters.subjectId = payload.subjectId
    bankStore.filters.chapterId = payload.chapterId
    bankStore.filters.page = 1
    await router.replace({
      path: route.path,
      query: {
        subjectId: payload.subjectId,
        ...(payload.chapterId ? { chapterId: payload.chapterId } : {}),
      },
    })
    return
  }

  if (['/questions/new', '/questions/document', '/word-import'].includes(route.path)) {
    if (!payload.subjectId) {
      ElMessage.warning('录入或导入题目时，请在左侧选择一个具体章节')
      return
    }
    const subject = appStore.subjects.find((entry) => entry.id === payload.subjectId)
    const chapter = payload.chapterId
      ? subject?.chapters.find((entry) => entry.id === payload.chapterId)
      : subject?.chapters[0]
    if (!subject || !chapter) {
      ElMessage.warning('该学科还没有可用章节，请先新增章节')
      return
    }
    bankStore.filters.subjectId = subject.id
    bankStore.filters.chapterId = chapter.id
    bankStore.filters.page = 1
    await router.replace({
      path: route.path,
      query: {
        ...route.query,
        subjectId: subject.id,
        chapterId: chapter.id,
      },
    })
    return
  }

  if (route.path !== '/questions') await router.push('/questions')
  bankStore.filters.subjectId = payload.subjectId
  bankStore.filters.chapterId = payload.chapterId
  bankStore.filters.page = 1
}
</script>

<template>
  <div class="app-page">
    <SubjectTree
      :selected-subject-id="bankStore.filters.subjectId"
      :selected-chapter-id="bankStore.filters.chapterId"
      @select="selectTree"
    />
    <router-view />
  </div>
</template>
