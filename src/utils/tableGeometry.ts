/** Bounded integer pixels shared by import, editing and paper composition. */
export function tablePixelSize(value: unknown): number | null {
  const number = Number(value)
  return Number.isFinite(number) && number > 0 && number <= 10000 ? Math.max(1, Math.round(number)) : null
}

export function fitTableColumns(values: readonly number[], total: number, minimum = 1): number[] {
  if (!values.length) return []
  const target = Math.max(values.length, Math.floor(total))
  const lower = Math.max(1, Math.min(Math.floor(minimum), Math.floor(target / values.length)))
  const weights = values.map((value) => tablePixelSize(value) ?? 1)
  const sum = (items: readonly number[]) => items.reduce((result, value) => result + value, 0)
  const weight = sum(weights)
  const result = weights.map((value) => Math.max(lower, value * target / weight))
  for (let attempt = 0; attempt <= values.length; attempt += 1) {
    const remainder = target - sum(result)
    if (Math.abs(remainder) < 0.01) break
    const flexible = result.map((value, index) => ({ value, index }))
      .filter(({ value }) => remainder > 0 || value > lower + 0.01)
    if (!flexible.length) break
    flexible.forEach(({ index }) => { result[index] = Math.max(lower, result[index]! + remainder / flexible.length) })
  }
  const rounded = result.map(Math.floor)
  let remainder = target - sum(rounded)
  for (let index = 0; remainder > 0; index = (index + 1) % rounded.length) {
    rounded[index] = rounded[index]! + 1
    remainder -= 1
  }
  return rounded
}
