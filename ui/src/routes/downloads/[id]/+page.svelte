<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import type { DownloadCollection } from '$lib/api';
	import { openPlayer, toast } from '$lib/player.svelte';
	import { t } from '$lib/i18n.svelte';
	import { Button } from '$lib/components/ui/button';
	import TrackRow from '$lib/components/TrackRow.svelte';

	let collection = $state<DownloadCollection | null>(null);
	let loading = $state(true);

	onMount(async () => {
		try {
			collection = await api.getDownloadCollection(decodeURIComponent(page.params.id ?? ''));
		} catch (e) {
			toast.error(String(e));
		} finally {
			loading = false;
		}
	});

	async function playAll() {
		if (!collection?.items.length) return;
		openPlayer();
		await api.playPlaylist(collection.items, 0, undefined, collection.title);
	}
</script>

<div class="p-6">
	<Button variant="ghost" size="sm" onclick={() => goto('/library?tab=downloads')}>← {t('common.back')}</Button>
	{#if loading}
		<p class="mt-6 text-sm text-muted-foreground">{t('common.loading')}</p>
	{:else if collection}
		<div class="mb-6 mt-4 flex flex-wrap items-end justify-between gap-4">
			<div>
				<p class="text-xs uppercase text-muted-foreground">{collection.kind}</p>
				<h1 class="font-heading text-3xl font-bold">{collection.title}</h1>
				{#if collection.subtitle}<p class="mt-1 text-sm text-muted-foreground">{collection.subtitle}</p>{/if}
			</div>
			<Button onclick={playAll} disabled={!collection.items.length}>{t('common.play_all')}</Button>
		</div>
		<div class="divide-y rounded-xl border bg-card">
			{#each collection.items as song, index (song.video_id + ':' + index)}
				<TrackRow song={song} {index} onplay={() => api.play(song)} />
			{/each}
		</div>
	{:else}
		<p class="mt-6 text-sm text-muted-foreground">{t('downloads.empty')}</p>
	{/if}
</div>
