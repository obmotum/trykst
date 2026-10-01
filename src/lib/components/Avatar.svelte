<script lang="ts">
    /**
     * Profile picture from the identity provider, or coloured initials when there is
     * none or it fails to load.
     */
    let {
        name = '',
        url = null,
        seed = '',
        size = 32,
        class: className = ''
    }: { name?: string | null; url?: string | null; seed?: string; size?: number; class?: string } = $props();

    const COLORS = ['#1e3a5f', '#0f766e', '#7c2d12', '#4c1d95', '#9f1239', '#365314', '#1e40af', '#854d0e'];

    let failed = $state(false);
    $effect(() => {
        url;
        failed = false;
    });

    const initial = $derived((name || '?').trim().charAt(0).toUpperCase() || '?');
    const color = $derived.by(() => {
        let hash = 0;
        for (const ch of seed || name || '?') hash = (hash * 31 + ch.charCodeAt(0)) | 0;
        return COLORS[Math.abs(hash) % COLORS.length];
    });
</script>

{#if url && !failed}
    <img
        src={url}
        alt=""
        referrerpolicy="no-referrer"
        onerror={() => (failed = true)}
        class="rounded-full object-cover flex-shrink-0 {className}"
        style="width: {size}px; height: {size}px;"
    />
{:else}
    <span
        class="rounded-full flex items-center justify-center font-semibold text-white flex-shrink-0 select-none {className}"
        style="width: {size}px; height: {size}px; font-size: {Math.round(size * 0.42)}px; background-color: {color};"
        aria-hidden="true"
    >
        {initial}
    </span>
{/if}
