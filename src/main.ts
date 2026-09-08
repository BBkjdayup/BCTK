import { createApp } from 'vue'
import { createPinia } from 'pinia'
import 'element-plus/theme-chalk/el-message.css'
import 'element-plus/theme-chalk/el-message-box.css'
import 'element-plus/theme-chalk/el-loading.css'
import 'katex/dist/katex.min.css'
import './styles/global.css'
import App from './App.vue'
import router from './router'

const app = createApp(App)

app.use(createPinia())
app.use(router)
app.mount('#app')
