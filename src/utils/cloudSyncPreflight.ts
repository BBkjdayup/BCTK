import type { CloudSyncPreflight } from '../types/domain'

export interface CloudSyncPreflightCopy {
  title: string
  detail: string
  confirmText: string
}

export function cloudSyncPreflightCopy(preflight: CloudSyncPreflight): CloudSyncPreflightCopy {
  if (preflight.bothNonEmpty) {
    return {
      title: '确认智能合并',
      detail: `检测到本机有 ${preflight.localQuestionCount} 道题、云端有 ${preflight.cloudQuestionCount} 道题。首次同步将采用智能合并：不同题目全部保留，同名学科、章节、标签和相同图片自动归一，只有同一道题被双方修改时才需要确认。`,
      confirmText: '合并并同步',
    }
  }
  if (preflight.localHasData) {
    return {
      title: '确认首次同步',
      detail: `检测到本机有 ${preflight.localQuestionCount} 道题，云端目前为空；将以本机题库建立云端副本。`,
      confirmText: '绑定并同步',
    }
  }
  if (preflight.cloudHasData) {
    return {
      title: '确认首次同步',
      detail: `本机题库为空，云端有 ${preflight.cloudQuestionCount} 道题；将下载云端题库到本机。`,
      confirmText: '绑定并同步',
    }
  }
  return {
    title: '确认首次同步',
    detail: '本机和云端题库均为空，将建立安全的同步关系。',
    confirmText: '绑定并同步',
  }
}
