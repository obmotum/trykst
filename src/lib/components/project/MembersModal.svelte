<script lang="ts">
	import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import { userStore } from '$lib/ts/auth';
	import { api } from '$lib/ts/api';
	import type { Project, ProjectMember, Role } from '$lib/ts/api';

	let {
		project,
		onClose,
		onChanged,
		onLeft
	}: {
		project: Project;
		onClose: () => void;
		/** The member list changed (count, roles). */
		onChanged: () => void;
		/** The signed-in user left the project. */
		onLeft: () => void;
	} = $props();

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

	let isOwner = $derived(project.role === 'owner');
	let members = $state<ProjectMember[]>([]);
	let loading = $state(true);
	let busyId = $state<string | null>(null);
	let error = $state('');

	let query = $state('');
	let addRole = $state<Role>('editor');
	let adding = $state(false);
	let addMessage = $state('');
	let addFailed = $state(false);

	// People picker (directory search)
	const PAGE_SIZE = 5;
	let suggestions = $state<Person[]>([]);
	let highlighted = $state(0);
	let searchLimit = $state(PAGE_SIZE);
	let selectedPerson = $state<Person | null>(null);
	let searchTimer: ReturnType<typeof setTimeout> | undefined;

	function personName(p: Person) {
		return p.display_name || p.username;
	}

	async function runSearch(q: string, limit: number) {
		try {
			const res = await fetch(`/api/directory/search?q=${encodeURIComponent(q)}&limit=${limit}`);
			if (res.ok && query.trim() === q) {
				const memberIds = new Set(members.map((m) => m.user_id));
				suggestions = ((await res.json()) as Person[]).filter((p) => !p.user_id || !memberIds.has(p.user_id));
				highlighted = Math.min(highlighted, Math.max(suggestions.length - 1, 0));
			}
		} catch {
			suggestions = [];
		}
	}

	function onQueryInput() {
		selectedPerson = null;
		clearTimeout(searchTimer);
		searchLimit = PAGE_SIZE;
		highlighted = 0;
		const q = query.trim();
		if (q.length < 2) {
			suggestions = [];
			return;
		}
		searchTimer = setTimeout(() => runSearch(q, searchLimit), 250);
	}

	function showMore() {
		searchLimit += 10;
		runSearch(query.trim(), searchLimit);
	}

	function choosePerson(p: Person) {
		selectedPerson = p;
		query = personName(p);
		suggestions = [];
	}

	function onQueryKeydown(e: KeyboardEvent) {
		if (!suggestions.length) return;
		const visible = Math.min(suggestions.length, searchLimit);
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			highlighted = (highlighted + 1) % visible;
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			highlighted = (highlighted - 1 + visible) % visible;
		} else if (e.key === 'Enter') {
			e.preventDefault();
			choosePerson(suggestions[highlighted]);
		} else if (e.key === 'Escape') {
			e.stopPropagation();
			suggestions = [];
		}
	}

	async function loadMembers() {
		try {
			members = await api<ProjectMember[]>('GET', `/api/projects/${project.id}/members`);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load members';
		}
		loading = false;
	}

	async function addMember(e: Event) {
		e.preventDefault();
		if (!selectedPerson && !query.trim()) return;
		adding = true;
		addMessage = '';
		try {
			await api(
				'POST',
				`/api/projects/${project.id}/members`,
				selectedPerson?.subject
					? { subject: selectedPerson.subject, role: addRole }
					: { email: selectedPerson?.email ?? query.trim(), role: addRole }
			);
			addFailed = false;
			addMessage = 'Added to the project.';
			query = '';
			selectedPerson = null;
			suggestions = [];
			await loadMembers();
			onChanged();
		} catch (err) {
			addFailed = true;
			addMessage = err instanceof Error ? err.message : 'Failed to add this person';
		}
		adding = false;
	}

	async function changeRole(member: ProjectMember, select: HTMLSelectElement) {
		busyId = member.user_id;
		error = '';
		try {
			await api('PATCH', `/api/projects/${project.id}/members/${member.user_id}`, { role: select.value });
			await loadMembers();
			onChanged();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to change the role';
			// The role did not change: show the real one again.
			select.value = member.role;
		}
		busyId = null;
	}

	async function removeMember(member: ProjectMember) {
		const self = member.user_id === $userStore?.id;
		if (!confirm(self ? `Leave “${project.name}”? You lose access to all its documents.` : `Remove ${member.username} from “${project.name}”?`)) return;
		busyId = member.user_id;
		error = '';
		try {
			await api('DELETE', `/api/projects/${project.id}/members/${member.user_id}`);
			if (self) {
				onLeft();
				return;
			}
			await loadMembers();
			onChanged();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to remove this member';
		}
		busyId = null;
	}

	onMount(loadMembers);
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4" role="presentation" onclick={onClose}>
	<div tabindex="-1" class="rounded-xl shadow-2xl border w-full max-w-[540px] bg-[var(--theme-bg)] text-[var(--theme-text)] border-[var(--theme-border)]" role="dialog" aria-modal="true" aria-labelledby="members-dialog-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') onClose(); }}>
		<div class="flex justify-between items-center p-4 border-b border-[var(--theme-border)]">
			<h2 id="members-dialog-title" class="text-lg font-semibold truncate">Members of “{project.name}”</h2>
			<button onclick={onClose} aria-label="Close" class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-full p-1 transition-colors">
				<Icon icon="mdi:close" class="text-xl" />
			</button>
		</div>

		<div class="p-6 space-y-5">
			{#if isOwner}
				<div class="space-y-2" data-invite>
					<div class="relative">
						<form onsubmit={addMember} class="flex items-center gap-2 bg-gray-50 dark:bg-zinc-900/50 p-1.5 rounded-lg border border-gray-300 dark:border-zinc-700 focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500 transition-all">
							<div class="pl-2 text-gray-400"><Icon icon="mdi:account-plus-outline" class="text-xl" /></div>
							<input
								type="text"
								placeholder="Add people by name or email..."
								aria-label="Add people by name or email"
								bind:value={query}
								oninput={onQueryInput}
								onkeydown={onQueryKeydown}
								onblur={() => setTimeout(() => (suggestions = []), 150)}
								autocomplete="off"
								required
								class="flex-1 bg-transparent border-none text-gray-800 dark:text-gray-200 text-sm px-2 py-2 focus:ring-0 focus:outline-none w-full"
							/>
							<div class="h-6 w-px bg-gray-300 dark:bg-zinc-700"></div>
							<select bind:value={addRole} aria-label="Role" class="bg-transparent border-none text-sm text-gray-700 dark:text-gray-300 px-2 py-2 focus:ring-0 focus:outline-none cursor-pointer font-medium">
								<option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="viewer">Viewer</option>
								<option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="editor">Editor</option>
								<option class="bg-white dark:bg-zinc-800 text-gray-900 dark:text-gray-100" value="owner">Owner</option>
							</select>
							<button type="submit" disabled={adding} class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors shadow-sm disabled:opacity-70 min-w-[70px]">
								{adding ? 'Adding...' : 'Add'}
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
											class="w-full text-left px-3 py-2 flex items-center gap-3 rounded-md border transition-colors {index === highlighted ? 'border-gray-800 dark:border-gray-100 bg-gray-100 dark:bg-white/10' : 'border-transparent'}"
										>
											<Avatar name={personName(person)} url={person.picture} seed={person.subject ?? person.user_id ?? person.username} size={36} />
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
											<Icon icon="mdi:magnify" class="text-lg" /> Show more results
										</button>
									</div>
								{/if}
							</div>
						{/if}
					</div>
					{#if addMessage}
						<div class="flex items-center gap-1.5 text-xs font-medium {addFailed ? 'text-red-600 dark:text-red-400' : 'text-emerald-600 dark:text-emerald-400'}" role="status">
							<Icon icon={addFailed ? 'mdi:alert-circle' : 'mdi:check-circle'} class="text-sm" />
							{addMessage}
						</div>
					{/if}
				</div>
			{/if}

			<div class="space-y-1">
				<div class="text-sm font-semibold">People with access</div>
				{#if loading}
					<div class="flex items-center gap-2 text-sm text-gray-400 py-2"><Icon icon="mdi:loading" class="animate-spin text-base" /> Loading members...</div>
				{:else}
					<div class="max-h-[40vh] overflow-y-auto -mx-1 px-1">
						{#each members as member (member.user_id)}
							{@const self = member.user_id === $userStore?.id}
							<div class="flex items-center gap-3 py-2">
								<Avatar name={member.username} url={member.avatar_url} seed={member.user_id} size={32} />
								<div class="flex-1 min-w-0">
									<p class="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
										{member.username}{#if self}<span class="text-gray-500 dark:text-gray-400 font-normal"> (you)</span>{/if}
										{#if member.is_guest}<span class="ml-1.5 text-xs px-1.5 py-px rounded bg-purple-100 dark:bg-purple-500/20 text-purple-700 dark:text-purple-300 font-normal">Guest</span>{/if}
									</p>
									<p class="text-xs text-gray-500 dark:text-gray-400 truncate">{member.email}</p>
								</div>
								{#if isOwner}
									<select
										value={member.role}
										disabled={busyId === member.user_id}
										onchange={(e) => changeRole(member, e.currentTarget)}
										aria-label="Role of {member.username}"
										class="flex-shrink-0 bg-gray-100 dark:bg-zinc-800 border border-gray-200 dark:border-zinc-700 text-xs font-medium text-gray-700 dark:text-gray-300 rounded-md px-2 py-1 cursor-pointer disabled:opacity-60"
									>
										<option value="viewer">Viewer</option>
										<option value="editor">Editor</option>
										<option value="owner">Owner</option>
									</select>
								{:else}
									<span class="flex-shrink-0 text-xs font-semibold px-2 py-0.5 rounded-full capitalize {member.role === 'owner' ? 'bg-amber-100 dark:bg-amber-500/20 text-amber-700 dark:text-amber-300' : member.role === 'editor' ? 'bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400'}">{member.role}</span>
								{/if}
								{#if isOwner || self}
									<button
										onclick={() => removeMember(member)}
										disabled={busyId === member.user_id}
										title={self ? 'Leave project' : 'Remove from project'}
										aria-label={self ? 'Leave project' : `Remove ${member.username}`}
										class="flex-shrink-0 p-1 rounded text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 transition-colors disabled:opacity-40"
									>
										<Icon icon={self ? 'mdi:logout' : 'mdi:close'} class="text-base" />
									</button>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
				{#if error}
					<p class="text-sm text-red-600 dark:text-red-400 pt-1" role="alert">{error}</p>
				{/if}
			</div>

			<p class="text-xs text-gray-500 dark:text-gray-400">
				Owners manage members and can delete the project. Editors create and edit documents. Viewers read.
			</p>
		</div>

		<div class="p-4 rounded-b-xl bg-gray-50 dark:bg-zinc-950/50 border-t border-[var(--theme-border)] flex justify-end">
			<button onclick={onClose} class="px-6 py-2 text-sm font-semibold text-white bg-gray-800 hover:bg-gray-900 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white rounded-lg shadow-sm transition-colors">Done</button>
		</div>
	</div>
</div>
