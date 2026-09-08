export function chunked<T>(entries: readonly T[], size: number): T[][] {
  if (!Number.isSafeInteger(size) || size < 1) throw new Error('批处理大小必须是正整数。')
  const chunks: T[][] = []
  for (let index = 0; index < entries.length; index += size) {
    chunks.push(entries.slice(index, index + size))
  }
  return chunks
}

export async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timeout = setTimeout(() => reject(new Error(message)), timeoutMs)
      }),
    ])
  } finally {
    clearTimeout(timeout)
  }
}
