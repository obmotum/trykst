<script lang="ts">
    import { onMount } from 'svelte';
    import { page } from '$app/stores';
    import Icon from '@iconify/svelte';
    import { userStore } from '$lib/ts/auth';
    
    let { onClose, docId = undefined } = $props<{ onClose: () => void, docId?: string }>();

    type CollaboratorView = { id: string; user_id: string; username: string; email: string | null; role: string; created_at: string };
    type Person = {
        subject: string | null;
        user_id: string | null;
        username: string;
        email: string | null;
        display_name: string | null;
        organization: string | null;
        picture: string | null;
        is_guest: boolean;
    };

    let link = $state('');
    let copied = $state(false);
    let role = $state('editor');

    let collaborators = $state<CollaboratorView[]>([]);
    let collabLoading = $state(false);
    let removingId = $state<string | null>(null);

    let inviteEmail = $state('');
    let inviteRole = $state('editor');
    let inviteStatus = $state<'idle' | 'loading' | 'success' | 'error'>('idle');
    let inviteMessage = $state('');

    // People picker (directory search)
    const PAGE_SIZE = 5;
    let suggestions = $state<Person[]>([]);
    let highlighted = $state(0);
    let searchLimit = $state(PAGE_SIZE);
    let selectedPerson = $state<Person | null>(null);
    let searchTimer: ReturnType<typeof setTimeout> | undefined;

    const AVATAR_COLORS = ['#1e3a5f', '#0f766e', '#7c2d12', '#4c1d95', '#9f1239', '#365314', '#1e40af', '#854d0e'];

    function personName(p: Person) {
        return p.display_name || p.username;
    }

    function avatarColor(p: Person) {
        const key = p.subject ?? p.user_id ?? p.username;
        let hash = 0;
        for (const ch of key) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
        return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
    }

    async function runSearch(q: string, limit: number) {
        try {
            const res = await fetch(`/api/directory/search?q=${encodeURIComponent(q)}&limit=${limit}`);
            if (res.ok && inviteEmail.trim() === q) {
                suggestions = await res.json();
                highlighted = Math.min(highlighted, Math.max(suggestions.length - 1, 0));
            }
        } catch {
            suggestions = [];
        }
    }

    function onInviteInput() {
        selectedPerson = null;
        clearTimeout(searchTimer);
        searchLimit = PAGE_SIZE;
        highlighted = 0;
        const q = inviteEmail.trim();
        if (q.length < 2) { suggestions = []; return; }
        searchTimer = setTimeout(() => runSearch(q, searchLimit), 250);
    }

    function showMore() {
        searchLimit += 10;
        runSearch(inviteEmail.trim(), searchLimit);
    }

    function choosePerson(p: Person) {
        selectedPerson = p;
        inviteEmail = personName(p);
        suggestions = [];
    }

    function onInviteKeydown(e: KeyboardEvent) {
        if (!suggestions.length) return;
        if (e.key === 'ArrowDown') {
            e.preventDefault();
            highlighted = (highlighted + 1) % Math.min(suggestions.length, searchLimit);
        } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            const visible = Math.min(suggestions.length, searchLimit);
            highlighted = (highlighted - 1 + visible) % visible;
        } else if (e.key === 'Enter') {
            e.preventDefault();
            choosePerson(suggestions[highlighted]);
        } else if (e.key === 'Escape') {
            e.stopPropagation();
            suggestions = [];
        }
    }
    
    async function loadCollaborators() {
        if (!docId) return;
        collabLoading = true;
        try {
            const res = await fetch(`/api/docs/${docId}/collaborators`);
            if (res.ok) collaborators = await res.json();
        } catch {}
        collabLoading = false;
    }

    async function removeCollaborator(collab: CollaboratorView) {
        if (!docId) return;
        removingId = collab.id;
        try {
            const res = await fetch(`/api/docs/${docId}/collaborators/${collab.id}`, { method: 'DELETE' });
            if (res.ok) collaborators = collaborators.filter(c => c.id !== collab.id);
        } catch {}
        removingId = null;
    }

    async function inviteUser(e: Event) {
        e.preventDefault();
        if (!docId || (!selectedPerson && !inviteEmail.trim())) return;

        inviteStatus = 'loading';
        inviteMessage = '';

        try {
            const res = await fetch(`/api/docs/${docId}/invite`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(
                    selectedPerson?.subject
                        ? { subject: selectedPerson.subject, role: inviteRole }
                        : { email: selectedPerson?.email ?? inviteEmail.trim(), role: inviteRole }
                )
            });

            if (res.ok) {
                inviteStatus = 'success';
                inviteMessage = 'User invited successfully!';
                inviteEmail = '';
                selectedPerson = null;
                suggestions = [];
                loadCollaborators();
            } else {
                const text = await res.text();
                inviteStatus = 'error';
                inviteMessage = text || 'Failed to invite user';
            }
        } catch (err) {
            inviteStatus = 'error';
            inviteMessage = 'Network error occurred';
        }
    }

    onMount(() => {
        const baseUrl = window.location.origin;
        const docUrl = docId ? `${baseUrl}/doc/${docId}` : window.location.href;
        link = `${docUrl}?role=${role}`;
        loadCollaborators();
    });
    
    $effect(() => {
        const baseUrl = window.location.origin;
        
        let docUrl = docId ? `${baseUrl}/doc/${docId}` : window.location.href.split('?')[0];
        link = `${docUrl}?role=${role}`;
    });
    
    function copyLink() {
        if (navigator.clipboard && window.isSecureContext) {
            navigator.clipboard.writeText(link).catch(console.error);
        } else {
            const input = document.getElementById('share-link-input') as HTMLInputElement;
            if (input) {
                input.select();
                try {
                    document.execCommand('copy');
                } catch (err) {
                    console.error('Fallback copy failed', err);
                }
            }
        }
        copied = true;
        setTimeout(() => copied = false, 2000);
    }
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4 animate-in fade-in duration-200" role="presentation" onclick={onClose}>
    <div tabindex="-1" class="rounded-xl shadow-2xl border w-full max-w-[500px] overflow-visible bg-[var(--theme-bg)] text-[var(--theme-text)] border-[var(--theme-border)]" style="background-color: var(--theme-bg); color: var(--theme-text); border-color: var(--theme-border);" role="dialog" aria-modal="true" aria-labelledby="share-dialog-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') onClose(); }}>
        <div class="flex justify-between items-center p-4 border-b border-[var(--theme-border)]" style="border-color: var(--theme-border);">
            <h2 id="share-dialog-title" class="text-lg font-semibold text-[var(--theme-text)]" style="color: var(--theme-text);">Share Document</h2>
            <button onclick={onClose} aria-label="Close" class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-full p-1 transition-colors">
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
            </button>
        </div>
        
        <div class="p-6 space-y-6">
            {#if !$userStore?.is_guest}
            <div class="space-y-3" data-invite>
                <div class="text-sm font-semibold text-[var(--theme-text)]" style="color: var(--theme-text);">Invite Collaborator</div>
                <div class="relative">
                <form onsubmit={inviteUser} class="flex items-center gap-2 bg-gray-50 dark:bg-zinc-900/50 p-1.5 rounded-lg border border-gray-300 dark:border-zinc-700 focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500 transition-all">
                    <div class="pl-2 text-gray-400">
                        <Icon icon="mdi:account-plus-outline" class="text-xl" />
                    </div>
                    <input 
                        type="text"
                        placeholder="Add people by name or email..."
                        bind:value={inviteEmail}
                        oninput={onInviteInput}
                        onkeydown={onInviteKeydown}
                        onblur={() => setTimeout(() => (suggestions = []), 150)}
                        autocomplete="off"
                        required
                        class="flex-1 bg-transparent border-none text-gray-800 dark:text-gray-200 text-sm px-2 py-2 focus:ring-0 focus:outline-none w-full"
                    />
                    <div class="h-6 w-px bg-gray-300 dark:bg-zinc-700"></div>
                    <select bind:value={inviteRole} class="bg-transparent border-none text-sm text-gray-700 dark:text-gray-300 px-2 py-2 focus:ring-0 focus:outline-none cursor-pointer font-medium">
                        <option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="editor">Editor</option>
                        <option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="viewer">Viewer</option>
                    </select>
                    <button 
                        type="submit"
                        disabled={inviteStatus === 'loading'}
                        class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors shadow-sm disabled:opacity-70 min-w-[80px]"
                    >
                        {inviteStatus === 'loading' ? 'Inviting...' : 'Invite'}
                    </button>
                </form>
                {#if suggestions.length > 0}
                    <div class="absolute left-0 right-0 mt-1 z-10 rounded-lg border border-gray-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 shadow-xl p-1" role="listbox">
                        <div class="max-h-80 overflow-y-auto">
                            {#each suggestions.slice(0, searchLimit) as person, index (person.subject ?? person.user_id)}
                                <button
                                    type="button"
                                    role="option"
                                    aria-selected={index === highlighted}
                                    onmousedown={(e) => e.preventDefault()}
                                    onclick={() => choosePerson(person)}
                                    onmouseenter={() => (highlighted = index)}
                                    class="w-full text-left px-3 py-2 flex items-center gap-3 rounded-md border transition-colors {index === highlighted
                                        ? 'border-gray-800 dark:border-gray-100 bg-gray-100 dark:bg-white/10'
                                        : 'border-transparent'}"
                                >
                                    {#if person.picture}
                                        <img src={person.picture} alt="" class="h-9 w-9 rounded-full object-cover flex-shrink-0" />
                                    {:else}
                                        <span class="h-9 w-9 rounded-full flex items-center justify-center text-sm font-semibold text-white flex-shrink-0" style="background-color: {avatarColor(person)}">
                                            {personName(person)[0]?.toUpperCase()}
                                        </span>
                                    {/if}
                                    <span class="min-w-0 flex-1">
                                        <span class="block text-sm text-gray-900 dark:text-gray-100 truncate">
                                            {personName(person)}{#if person.organization}<span class="text-gray-500 dark:text-gray-400">{` | ${person.organization}`}</span>{/if}
                                        </span>
                                        <span class="block text-xs text-gray-500 dark:text-gray-400 truncate">
                                            {person.email ?? person.username}{#if !person.user_id}<span class="italic">{' · not signed in yet'}</span>{/if}{#if person.is_guest}<span class="ml-1.5 px-1.5 py-px rounded bg-purple-100 dark:bg-purple-500/20 text-purple-700 dark:text-purple-300 not-italic">Guest</span>{/if}
                                        </span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                        {#if suggestions.length > searchLimit}
                            <div class="border-t border-gray-200 dark:border-zinc-700 mt-1 pt-1">
                                <button type="button" onmousedown={(e) => e.preventDefault()} onclick={showMore} class="w-full text-left px-3 py-2 flex items-center gap-3 rounded-md text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-white/10">
                                    <Icon icon="mdi:magnify" class="text-lg" />
                                    Show more results
                                </button>
                            </div>
                        {/if}
                    </div>
                {/if}
                </div>
                {#if inviteMessage}
                    <div class="flex items-center gap-1.5 text-xs font-medium {inviteStatus === 'success' ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'}">
                        <Icon icon={inviteStatus === 'success' ? 'mdi:check-circle' : 'mdi:alert-circle'} class="text-sm" />
                        {inviteMessage}
                    </div>
                {/if}
            </div>
            {/if}

            {#if collabLoading}
                <div class="flex items-center gap-2 text-sm text-gray-400 dark:text-gray-500 py-1">
                    <Icon icon="mdi:loading" class="animate-spin text-base" />
                    Loading collaborators...
                </div>
            {:else if collaborators.length > 0}
                <div class="h-px bg-gray-200 dark:bg-zinc-800/50"></div>
                <div class="space-y-2">
                    <div class="text-sm font-semibold text-[var(--theme-text)]" style="color: var(--theme-text);">People with access</div>
                    {#each collaborators as collab (collab.id)}
                        <div class="flex items-center gap-3 py-1.5">
                            <div class="w-8 h-8 rounded-full bg-blue-100 dark:bg-blue-900/30 flex items-center justify-center text-blue-600 dark:text-blue-400 text-sm font-bold flex-shrink-0">
                                {collab.username[0].toUpperCase()}
                            </div>
                            <div class="flex-1 min-w-0">
                                <p class="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{collab.username}</p>
                                <p class="text-xs text-gray-500 dark:text-gray-400 truncate">{collab.email}</p>
                            </div>
                            <span class="text-xs font-semibold px-2 py-0.5 rounded-full flex-shrink-0 {collab.role === 'editor' ? 'bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400'}">
                                {collab.role}
                            </span>
                            <button
                                onclick={() => removeCollaborator(collab)}
                                disabled={removingId === collab.id}
                                title="Remove collaborator"
                                class="flex-shrink-0 p-1 rounded text-gray-400 hover:text-red-500 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-500/10 transition-colors disabled:opacity-40"
                            >
                                {#if removingId === collab.id}
                                    <Icon icon="mdi:loading" class="text-base animate-spin" />
                                {:else}
                                    <Icon icon="mdi:close" class="text-base" />
                                {/if}
                            </button>
                        </div>
                    {/each}
                </div>
            {/if}

            <div class="h-px bg-gray-200 dark:bg-zinc-800/50"></div>

            <div class="space-y-3">
                <div class="text-sm font-semibold text-[var(--theme-text)]" style="color: var(--theme-text);">General Access</div>
                <div class="flex items-center gap-4 p-3 bg-gray-50/50 dark:bg-zinc-950/30 rounded-xl border border-gray-200 dark:border-zinc-800/50 hover:bg-gray-50 dark:hover:bg-zinc-900/50 transition-colors">
                    <div class="bg-gray-200 dark:bg-zinc-800 p-2.5 rounded-full text-gray-600 dark:text-gray-300">
                        <Icon icon="mdi:earth" class="text-xl" />
                    </div>
                    <div class="flex-1">
                        <h4 class="text-sm font-medium text-gray-900 dark:text-white">Anyone with the link</h4>
                        <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">Can view and collaborate based on role</p>
                    </div>
                    <select bind:value={role} class="bg-gray-100 dark:bg-zinc-800 border border-gray-200 dark:border-zinc-700 text-sm font-medium text-gray-700 dark:text-gray-300 rounded-md px-3 py-1.5 focus:outline-none cursor-pointer focus:ring-2 focus:ring-blue-500/20 hover:bg-gray-200 dark:hover:bg-zinc-700 transition-colors">
                        <option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="viewer">Viewer</option>
                        <option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="editor">Editor</option>
                    </select>
                </div>
            </div>
        </div>
        
        <div class="p-4 rounded-b-xl bg-gray-50 dark:bg-zinc-950/50 border-t border-[var(--theme-border)] flex items-center justify-between" style="border-color: var(--theme-border);">
            <button 
                onclick={copyLink}
                class="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-blue-600 hover:bg-blue-50 dark:text-blue-400 dark:hover:bg-blue-500/10 transition-colors"
            >
                {#if copied}
                    <Icon icon="mdi:check" class="text-lg" />
                    <span>Link copied!</span>
                {:else}
                    <Icon icon="mdi:link-variant" class="text-lg" />
                    <span>Copy link</span>
                {/if}
            </button>

            <button onclick={onClose} class="px-6 py-2 text-sm font-semibold text-white bg-gray-800 hover:bg-gray-900 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white rounded-lg shadow-sm transition-colors">
                Done
            </button>
        </div>
    </div>
</div>
