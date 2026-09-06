import type { AppInfo } from '../types'

let queue: AppInfo[] = $state([])

export function getQueue() { return queue }
export function setQueue(apps: AppInfo[]) { queue = apps }
export function enqueue(app: AppInfo) { if (!queue.find(a => a.id === app.id)) queue = [...queue, app] }
export function dequeue() { const [first, ...rest] = queue; queue = rest; return first ?? null }
export function peek() { return queue[0] ?? null }
export function clearQueue() { queue = [] }
export function queueLength() { return queue.length }
