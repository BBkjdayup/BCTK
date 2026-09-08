import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      component: () => import('../layouts/QuestionBankWorkspace.vue'),
      children: [
        { path: '', redirect: '/questions' },
        { path: 'questions', name: 'questions', component: () => import('../views/QuestionBankView.vue') },
        { path: 'duplicate-check', name: 'duplicate-check', component: () => import('../views/DuplicateCheckView.vue') },
        { path: 'questions/new', name: 'question-new', component: () => import('../views/QuestionEditorView.vue') },
        { path: 'questions/document', name: 'question-document', component: () => import('../views/DocumentQuestionEditorView.vue') },
        { path: 'word-import', name: 'word-import', component: () => import('../views/WordImportView.vue') },
        { path: 'recycle-bin', name: 'recycle-bin', component: () => import('../views/RecycleBinView.vue') },
        { path: 'taxonomy', name: 'taxonomy', component: () => import('../views/TaxonomyView.vue') },
        { path: 'tags', name: 'tags', component: () => import('../views/TagsView.vue') },
        { path: 'question-types', name: 'question-types', component: () => import('../views/QuestionTypesView.vue') },
      ],
    },
    { path: '/questions/:id/edit', name: 'question-edit', component: () => import('../views/QuestionEditorView.vue') },
    { path: '/papers', name: 'papers', component: () => import('../views/PaperBuilderView.vue') },
    { path: '/history', name: 'history', component: () => import('../views/HistoryView.vue') },
    { path: '/templates', name: 'templates', component: () => import('../views/TemplatesView.vue') },
    { path: '/data', name: 'data', component: () => import('../views/DataManagementView.vue') },
    { path: '/statistics', name: 'statistics', component: () => import('../views/StatisticsView.vue') },
    { path: '/settings', name: 'settings', component: () => import('../views/SettingsView.vue') },
  ],
  scrollBehavior: () => ({ top: 0 }),
})

export default router
