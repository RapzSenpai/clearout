// ponytail: pure helpers beat duplicated review logic; ceiling is no framework, upgrade is shared review store.
import type { LeftoverItem } from '../types'

export function errText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === 'string' ? e : JSON.stringify(e)
}

export function mergeList(existing: LeftoverItem[], added: LeftoverItem[]): LeftoverItem[] {
  const ids = new Set(existing.map((i) => i.id))
  return [...existing, ...added.filter((i) => !ids.has(i.id))]
}

export function isOverridableRegistryPath(path: string): boolean {
  return /\\microsoft\\windows\\currentversion\\(uninstall|run|runonce)\\[^\\]+/i.test(path)
}

export function needsTypedConfirm(count: number, threshold = 20): boolean {
  return count >= threshold
}
