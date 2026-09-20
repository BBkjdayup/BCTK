import qrCode from '../assets/support-contact.jpg'

export interface SupportContact {
  display_name: string
  wechat_id: string
  service_hours: string
  qr_code: string
  version: number
}
export const defaultSupportContact: SupportContact = {
  display_name: 'TK试题题库', wechat_id: 'Ai_chatgpt_01', service_hours: '', qr_code: qrCode, version: 0,
}
export function parseSupportContact(value: unknown): SupportContact {
  const data = value as Partial<SupportContact> | null
  if (!data || typeof data.display_name !== 'string' || !data.display_name.trim() || data.display_name.length > 80
    || typeof data.wechat_id !== 'string' || !/^[A-Za-z][A-Za-z0-9_-]{5,19}$/.test(data.wechat_id)
    || typeof data.service_hours !== 'string' || data.service_hours.length > 160
    || typeof data.qr_code !== 'string' || data.qr_code.length > 1_398_128
    || (data.qr_code !== '' && !/^data:image\/(?:jpeg|png);base64,[A-Za-z0-9+/]+={0,2}$/.test(data.qr_code))
    || !Number.isInteger(data.version) || Number(data.version) < 1) throw new Error('客服信息格式无效')
  return data as SupportContact
}
