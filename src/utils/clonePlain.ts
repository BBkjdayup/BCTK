/**
 * Clone a JSON-compatible data-transfer object into plain data.
 *
 * Vue and Pinia wrap state in Proxy objects. Browser `structuredClone` rejects
 * those proxies, while all data sent to the Rust commands is intentionally
 * JSON-compatible.
 */
export function clonePlain<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}
