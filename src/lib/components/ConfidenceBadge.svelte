<script lang="ts">
  import type { ConfidenceTier } from '../types'

  let { tier }: { tier: ConfidenceTier } = $props()
</script>

<span class="badge badge-{tier.toLowerCase()}">
  {tier}
</span>

<style>
  .badge {
    display: inline-block;
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .badge-high {
    background-color: var(--color-accent);
    color: white;
  }

  .badge-medium {
    background-color: transparent;
    border: 1px solid var(--color-accent);
    color: var(--color-accent);
  }

  .badge-low {
    background-color: transparent;
    border: 1px solid var(--color-text-secondary);
    color: var(--color-text-secondary);
  }
</style>
