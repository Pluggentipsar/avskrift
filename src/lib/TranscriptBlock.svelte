<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  let { root, index, forced, estimate, children }: { root: HTMLElement; index: number; forced: boolean; estimate: number; children: Snippet } = $props();
  let shell: HTMLDivElement;
  let visible = $state(false), focused = $state(false), height = $state(0);
  const expanded = $derived(visible || forced || focused);
  // Width, font size and text changes invalidate cached heights.
  $effect(() => { void estimate; height = 0; });
  onMount(() => {
    const observer = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; }, { root, rootMargin: '500px 0px' });
    observer.observe(shell);
    return () => observer.disconnect();
  });
  function measure(node: HTMLElement) {
    const observer = new ResizeObserver(() => { height = node.getBoundingClientRect().height; });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>
<!-- Focused/editing content is retained even when scrolled outside the viewport. -->
<div bind:this={shell} data-block={index} style:min-height={expanded ? undefined : `${height || estimate}px`}
  onfocusin={() => focused = true} onfocusout={() => queueMicrotask(() => { focused = shell.contains(document.activeElement); })}>
  {#if expanded}<div use:measure>{@render children()}</div>{/if}
</div>
