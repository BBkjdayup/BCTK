<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import type { Question, QuestionSummary, QuestionHistoryEntry } from '../types/domain'
import RichContentView from './RichContentView.vue'
const props=defineProps<{modelValue:boolean;question:QuestionSummary|null}>()
const emit=defineEmits<{'update:modelValue':[value:boolean];restored:[]}>()
const entries=ref<QuestionHistoryEntry[]>([]),selected=ref<Question|null>(null),selectedId=ref(''),busy=ref(false),error=ref('')
let revision=0
onBeforeUnmount(()=>revision++)
watch(()=>[props.modelValue,props.question?.id],async()=>{
  const token=++revision;selected.value=null;selectedId.value='';entries.value=[];error.value=''
  if(!props.modelValue||!props.question)return
  busy.value=true
  try{const list=await backend.listQuestionHistory(props.question.id);if(token===revision)entries.value=list}catch(e){if(token===revision)error.value=errorMessage(e,'历史读取失败')}finally{if(token===revision)busy.value=false}
})
async function show(entry:QuestionHistoryEntry){const token=++revision;busy.value=true;error.value='';try{const detail=await backend.getQuestionHistory(entry.id);if(token===revision){selected.value=detail;selectedId.value=entry.id}}catch(e){if(token===revision)error.value=errorMessage(e,'读取失败')}finally{if(token===revision)busy.value=false}}
async function restore(){
  if(!props.question||!selected.value||busy.value)return
  const expected=props.question.contentVersion,id=selectedId.value
  try{await ElMessageBox.confirm(`将历史版本 ${selected.value.contentVersion} 的题干、答案、解析、分类和标签恢复为新版本？现有试卷保持原快照。`,'恢复题目',{confirmButtonText:'恢复为新版本'})}catch{return}
  busy.value=true
  try{await backend.restoreQuestionHistory(id,expected);emit('restored');emit('update:modelValue',false);ElMessage.success('历史内容已恢复为新版本')}catch(e){error.value=errorMessage(e,'恢复未执行')}finally{busy.value=false}
}
</script>
<template><el-dialog :model-value="modelValue" title="题目历史版本" width="900px" @update:model-value="emit('update:modelValue',$event)"><p>保留最近 50 个已保存版本及其图片。历史从本功能启用后的首次编辑开始积累；永久删除题目时一并删除历史。</p><el-alert v-if="error" :title="error" type="error" :closable="false"/><div class="history-layout" v-loading="busy"><aside><button v-for="entry in entries" :key="entry.id" :class="{active:entry.id===selectedId}" @click="show(entry)">版本 {{entry.contentVersion}}<small>{{new Date(entry.savedAt).toLocaleString()}}</small></button><el-empty v-if="!entries.length&&!busy" description="尚无历史记录" :image-size="60"/></aside><article v-if="selected"><div>{{selected.subjectName}} · {{selected.chapterName}} · {{selected.tags.map(t=>t.name).join('、')}}</div><h4>题干</h4><RichContentView :content="selected.stem"/><div v-for="(option,index) in selected.options" :key="option.id"><b>{{String.fromCharCode(65+index)}}.</b><RichContentView :content="option.content"/></div><h4>答案</h4><RichContentView :content="selected.answer"/><h4>解析</h4><RichContentView :content="selected.explanation"/></article><el-empty v-else description="选择左侧版本查看内容"/></div><template #footer><el-button @click="emit('update:modelValue',false)">关闭</el-button><el-button type="primary" :disabled="busy||!selected||selected.contentVersion===question?.contentVersion" @click="restore">恢复为新版本</el-button></template></el-dialog></template>
<style scoped>.history-layout{display:grid;grid-template-columns:200px 1fr;gap:24px;min-height:300px}.history-layout aside,.history-layout article{max-height:55vh;overflow:auto}.history-layout button{display:block;width:100%;padding:12px;text-align:left;border:1px solid #e2e8f0;background:#fff;margin-bottom:8px;border-radius:8px;cursor:pointer}.history-layout small{display:block;margin-top:8px}.history-layout button.active{border-color:#2563eb;background:#eff6ff}.history-layout article{min-width:0}</style>
