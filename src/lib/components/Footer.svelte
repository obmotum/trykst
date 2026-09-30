<script lang="ts">
    import Icon from '@iconify/svelte';
    let { sticky = true }: { sticky?: boolean } = $props();
</script>

<footer class="mt-auto py-6 text-center text-sm text-gray-500 dark:text-gray-400 border-t border-gray-200 dark:border-white/10 bg-[var(--theme-bg)] {sticky ? 'sticky bottom-0 z-10' : ''} w-full flex-shrink-0 transition-colors duration-200">
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex flex-col justify-center items-center gap-4">
        <p class="inline-flex items-center gap-2">
            <span class="font-semibold text-gray-700 dark:text-gray-300">Trykst</span>
            <span>·</span>
            <a
                href="https://github.com/SirBlobby/TypstDrive"
                target="_blank"
                rel="noopener noreferrer"
                class="inline-flex items-center gap-1 hover:text-gray-900 dark:hover:text-white transition-colors"
            >
                <Icon icon="mdi:github" class="text-base" />
                Based on TypstDrive by SirBlobby
            </a>
        </p>
    </div>
</footer>
