<script lang="ts">
  const assets = import.meta.glob('../assets/icons/*.svg', { eager: true, query: '?url', import: 'default' }) as Record<string, string>;
  export type IconName = 'document-text' | 'user' | 'magnifying-glass' | 'clock' | 'share' | 'arrow-down-tray' | 'arrow-left' | 'x-mark' | 'bars-3' | 'chevron-right' | 'lock-closed' | 'plus' | 'command-line' | 'shield-check' | 'adjustments-horizontal';
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
</script>
<span aria-hidden="true" style:width="{size}px" style:height="{size}px" style={`--icon: url("${assets[`../assets/icons/${name}.svg`]}")`}></span>
<style>span { display: inline-block; flex: 0 0 auto; background: currentColor; -webkit-mask-image: var(--icon); mask-image: var(--icon); -webkit-mask-size: contain; mask-size: contain; -webkit-mask-position: center; mask-position: center; -webkit-mask-repeat: no-repeat; mask-repeat: no-repeat; }</style>
