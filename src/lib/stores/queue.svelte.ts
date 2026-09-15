import type { AppInfo } from '../types'

let queue: AppInfo[] = $state([])

export function setQueue(apps: AppInfo[]) { queue = apps }
export function clearQueue() { queue = [] }
