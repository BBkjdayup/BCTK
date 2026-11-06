<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { miniCloud } from '../services/miniProgramCloud'
import { errorMessage } from '../services/errors'
interface Member { id:string; name:string; status:string; attempts:number; completed:number; lastActive:string|null }
interface Attempt { id:string; title:string; status:string; total:number; answered:number; correct:number; wrong:number; updatedAt:string; results:{index:number;correct:boolean|null;read:boolean;selfGraded:boolean}[] }
interface Feedback { id:string; name:string; title:string; index:number; version:number; category:string; detail:string; preview:string; questionId:string; resolved:boolean }
interface Report { totals:{members:number;started:number;completed:number}; members:Member[]; difficult:{questionId:string;title:string;preview:string;version:number;samples:number;wrong:number;objective:number;objectiveWrong:number;selfGraded:number;selfWrong:number;students:number}[]; attempts:Attempt[]; feedback:Feedback[] }
const report=ref<Report|null>(null), busy=ref(false), error=ref(''), page=ref(1), student=ref<Member|null>(null)
let generation=0
onBeforeUnmount(()=>generation++)
async function load() {
  const current=++generation;busy.value=true;error.value=''
  try { const value=await miniCloud.request<Report>('POST','/learning',{page:page.value,studentId:student.value?.id});if(current===generation)report.value=value }
  catch(reason){if(current===generation){report.value=null;error.value=errorMessage(reason,'学情读取失败，请确认云服务已升级')}}
  finally {if(current===generation)busy.value=false}
}
function select(member:Member|null){student.value=member;page.value=1;void load()}
async function resolve(item:Feedback){if(busy.value)return;busy.value=true;try{await miniCloud.request('PUT','/feedback/'+item.id,{resolved:!item.resolved});await load()}catch(reason){error.value=errorMessage(reason,'反馈状态未保存')}finally{busy.value=false}}
function move(delta:number){page.value=Math.max(1,page.value+delta);void load()}
const date=(value:string|null)=>value?new Date(value).toLocaleString():'尚未开始'
onMounted(load)
</script>
<template>
  <section class="surface learning-report" v-loading="busy">
    <header><div><h2>学习情况与题目反馈</h2><p>仅当前老师题库 · 完成人数表示至少完成一轮。未交卷、答错和未开始分别显示。</p></div><el-button @click="load" :disabled="busy">刷新</el-button></header>
    <el-alert v-if="error" :title="error" type="error" :closable="false" />
    <template v-if="report">
      <p class="totals">学员 {{report.totals.members}} 人 · 已开始 {{report.totals.started}} 人 · 已完成 {{report.totals.completed}} 人</p>
      <template v-if="student"><h3>{{student.name}} 的练习 <el-button link @click="select(null)">返回学员列表</el-button></h3><el-table :data="report.attempts" row-key="id"><el-table-column prop="title" label="练习" min-width="200"/><el-table-column label="进度" width="130"><template #default="{row}">{{row.answered}} / {{row.total}} 题</template></el-table-column><el-table-column label="状态" width="120"><template #default="{row}">{{row.status==='completed'?'已完成':row.status==='review'?'待自行对照':'未交卷'}}</template></el-table-column><el-table-column label="判题详情" min-width="280"><template #default="{row}"><span v-for="item in row.results" :key="item.index" class="result-chip">{{item.index+1}}：{{item.read?'阅读':item.correct===null?'未判定':item.correct?'对':'错'}}{{item.selfGraded?'（自评）':''}}</span></template></el-table-column></el-table></template>
      <el-table v-else :data="report.members" row-key="id"><el-table-column prop="name" label="学员"/><el-table-column label="状态"><template #default="{row}">{{row.attempts ? row.completed ? '已有完成记录' : '已开始，尚未完成' : '未开始'}}</template></el-table-column><el-table-column label="练习 / 完成"><template #default="{row}">{{row.attempts}} / {{row.completed}}</template></el-table-column><el-table-column label="最近活动" min-width="160"><template #default="{row}">{{date(row.lastActive)}}</template></el-table-column><el-table-column width="100"><template #default="{row}"><el-button link @click="select(row)">查看详情</el-button></template></el-table-column></el-table>
      <h3>较常出错的题目（最多 20 道）</h3><p>判题样本从新版上线后累积；同一学员多次作答分别计入。自评次数另列。</p>
      <el-table :data="report.difficult"><el-table-column prop="title" label="试卷"/><el-table-column label="题目与版本" min-width="220"><template #default="{row}">{{row.preview}}<small>发布版本 {{row.version}}</small></template></el-table-column><el-table-column prop="students" label="参与人数"/><el-table-column label="客观题错误 / 样本"><template #default="{row}">{{row.objectiveWrong}} / {{row.objective}}</template></el-table-column><el-table-column label="自评错误 / 样本"><template #default="{row}">{{row.selfWrong}} / {{row.selfGraded}}</template></el-table-column></el-table>
      <h3>学员题目反馈</h3><el-table :data="report.feedback" row-key="id"><el-table-column prop="name" label="学员" width="100"/><el-table-column label="对应内容" min-width="200"><template #default="{row}">{{row.title}} · 第 {{row.index+1}} 题 · 发布版本 {{row.version}}<small>{{row.preview}}</small></template></el-table-column><el-table-column label="类型" width="100"><template #default="{row}">{{({wording:'题干选项',answer:'参考答案',media:'图片公式'} as Record<string,string>)[row.category]}}</template></el-table-column><el-table-column prop="detail" label="说明" min-width="220"/><el-table-column width="120"><template #default="{row}"><el-button link :disabled="busy" @click="resolve(row)">{{row.resolved?'重新打开':'标记已处理'}}</el-button></template></el-table-column></el-table>
      <footer><el-button :disabled="busy||page===1" @click="move(-1)">上一页</el-button><span>第 {{page}} 页 · 每组最多 20 条</span><el-button :disabled="busy || (student ? report.attempts.length<20 : report.members.length<20) && report.feedback.length<20" @click="move(1)">下一页</el-button></footer>
    </template>
  </section>
</template>
<style scoped>
.learning-report{padding:24px;min-width:0}.learning-report header,.learning-report footer{display:flex;justify-content:space-between;align-items:center;gap:16px}.learning-report h2{margin:0}.learning-report h3{margin-top:28px}.learning-report p{color:var(--text-muted,#64748b);font-size:13px}.totals{padding:16px;background:var(--bg-muted,#f1f5f9);border-radius:12px}.learning-report small{display:block;color:var(--text-muted,#64748b);margin-top:4px}.result-chip{display:inline-block;margin:2px 10px 2px 0}.learning-report footer{margin-top:20px}
</style>
