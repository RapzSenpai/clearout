<script lang="ts">
  import type { LeftoverItem } from '../types'

  let { items, impacts }: {
    items: LeftoverItem[]
    impacts: Map<string, { keys: number; values: number }>
  } = $props()
</script>

<!-- ponytail: impact list beats inline protect markup; ceiling is counts only, upgrade is full subtree diff. -->
<div class="protect-paths">
  <ul>
    {#each items.slice(0, 10) as i (i.id)}
      {@const impact = impacts.get(i.id)}
      <li class="font-mono">{i.path}{#if impact} — {impact.keys} keys, {impact.values} values{/if}</li>
    {/each}
  </ul>
  {#if items.length > 10}
    <p class="dialog-desc">…and {items.length - 10} more.</p>
  {/if}
</div>

<style>
  .protect-paths {
    max-height: 132px;
    overflow-y: auto;
    overflow-x: auto;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: var(--color-bg);
    padding: 8px 12px;
    margin: 0 0 16px 0;
  }
  .protect-paths ul { margin: 0; padding: 0; list-style: none; }
  .protect-paths li { font-size: 11.5px; line-height: 1.7; white-space: nowrap; }
</style>
