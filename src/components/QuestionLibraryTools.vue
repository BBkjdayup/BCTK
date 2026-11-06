<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import type { QuestionFilters, SavedQuestionFilter, QuestionBatchReceipt } from '../types/domain'
const props=defineProps<{filters:QuestionFilters}>()
const emit=defineEmits<{apply:[filters:QuestionFilters];changed:[]}>()
const presets=ref<SavedQuestionFilter[]>([]), selected=ref(''), batches=ref<QuestionBatchReceipt[]>([]), undoOpen=ref(false),busy=ref(false)
async function refresh(){try{presets.value=await backend.listSavedQuestionFilters()}catch(e){ElMessage.error(errorMessage(e,'筛选条件读取失败'))}}
onMounted(refresh)
async function save(){
  let name:string
  try{const result=await ElMessageBox.prompt('保存当前筛选条件，列表会随题库内容自动更新。','保存常用筛选',{inputPattern:/^.{1,40}$/,inputErrorMessage:'请输入 1 至 40 字',confirmButtonText:'保存'});name=result.value.trim();if(!name)return
    if(presets.value.some(p=>p.name===name))await ElMessageBox.confirm('同名筛选已存在，替换其条件？','替换筛选')
  }catch{return}
  try{await backend.saveQuestionFilter(name,{...props.filters,tagIds:[...props.filters.tagIds],page:1});await refresh();ElMessage.success('筛选条件已保存')}catch(e){ElMessage.error(errorMessage(e,'保存失败'))}
}
function apply(id:string){const preset=presets.value.find(p=>p.id===id);if(preset)emit('apply',{...preset.filters,tagIds:[...preset.filters.tagIds],page:1,deleted:false})}
async function remove(){if(!selected.value)return;try{await backend.deleteQuestionFilter(selected.value);selected.value='';await refresh()}catch(e){ElMessage.error(errorMessage(e,'删除失败'))}}
async function openUndo(){try{batches.value=await backend.listQuestionBatches();undoOpen.value=true}catch(e){ElMessage.error(errorMessage(e,'批次读取失败'))}}
async function undo(item:QuestionBatchReceipt){
  if(busy.value)return
  try{await ElMessageBox.confirm(`恢复这次批量修改前的 ${item.count} 道题的分类、标签和使用状态？如果之后已有修改，整批将停止恢复。`,'撤销批量修改',{confirmButtonText:'恢复修改前状态'})}catch{return}
  busy.value=true
  try{await backend.undoQuestionBatch(item.id);batches.value=await backend.listQuestionBatches();emit('changed');ElMessage.success('已撤销该次批量修改')}catch(e){ElMessage.error(errorMessage(e,'撤销未执行'))}finally{busy.value=false}
}
</script>
<template>
  <div class="library-tools"><el-select v-model="selected" clearable placeholder="选择已保存的筛选" @change="apply" style="width:240px"><el-option v-for="preset in presets" :key="preset.id" :value="preset.id" :label="preset.name"/></el-select><el-button @click="save">保存当前筛选</el-button><el-button v-if="selected" link @click="remove">删除此筛选</el-button><el-button @click="openUndo">批量修改记录</el-button></div>
  <el-dialog v-model="undoOpen" title="批量修改记录" width="640px"><p>最近 20 个未撤销批次。撤销前会检查全部题目，保留之后的编辑和使用记录。</p><el-table :data="batches"><el-table-column label="时间"><template #default="{row}">{{new Date(row.createdAt).toLocaleString()}}</template></el-table-column><el-table-column prop="count" label="题目数"/><el-table-column width="120"><template #default="{row}"><el-button :disabled="busy" link @click="undo(row)">撤销这次修改</el-button></template></el-table-column></el-table></el-dialog>
</template>
<style scoped>.library-tools{display:flex;align-items:center;gap:12px;flex-wrap:wrap;margin:0 0 12px}.library-tools :deep(.el-button + .el-button){margin-left:0}</style>
