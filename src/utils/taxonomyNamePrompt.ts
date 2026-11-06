import { ElMessageBox } from 'element-plus'

export function normalizeName(value: string) {
  return value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
}

export async function askForName(title: string, initialValue = '', maximum = 100): Promise<string | null> {
  try {
    const result = await ElMessageBox.prompt('名称将显示在题库筛选和题目录入页面中。', title, {
      inputValue: initialValue,
      inputPlaceholder: '请输入名称',
      confirmButtonText: '保存',
      cancelButtonText: '取消',
      inputValidator: (value: string) => {
        const name = value.trim().normalize('NFKC').trim()
        if (!name) return '名称不能为空'
        return [...name].length <= maximum || `名称不能超过 ${maximum} 个字符`
      },
    })
    return result.value.trim().normalize('NFKC').trim()
  } catch {
    return null
  }
}
