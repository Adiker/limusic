<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import {
		downloads,
		loadDownloads,
		pickDownloadFolder,
		playSong,
		toast
	} from '$lib/player.svelte';
	import { t } from '$lib/i18n.svelte';
	import { Button } from './ui/button';

	onMount(loadDownloads);

	const active = $derived(
		downloads.library?.items.filter((item) => item.state !== 'completed') ?? []
	);
	const complete = $derived(
		downloads.library?.items.filter((item) => item.state === 'completed') ?? []
	);

	function bytes(value: number) {
		if (!value) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB', 'TB'];
		const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
		return `${(value / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
	}

	function percent(item: api.DownloadItem) {
		return item.sizeBytes > 0 ? Math.min(100, Math.round((item.downloadedBytes / item.sizeBytes) * 100)) : 0;
	}

	async function chooseFolder() {
		const path = await pickDownloadFolder(downloads.library?.parent);
		if (!path) return;
		try {
			await api.setDownloadParent(path);
			await loadDownloads();
			toast.success(t('downloads.folder_updated'));
		} catch (e) {
			toast.error(String(e));
		}
	}

	async function action(item: api.DownloadItem, kind: 'pause' | 'resume' | 'retry' | 'cancel' | 'remove') {
		try {
			if (kind === 'pause') await api.pauseDownload(item.song.video_id);
			if (kind === 'resume') await api.resumeDownload(item.song.video_id);
			if (kind === 'retry') await api.retryDownload(item.song.video_id);
			if (kind === 'cancel') await api.cancelDownload(item.song.video_id);
			if (kind === 'remove') await api.removeDownload(item.song.video_id);
			await loadDownloads();
		} catch (e) {
			toast.error(String(e));
		}
	}

	async function removeAll() {
		if (!confirm(t('downloads.remove_all_confirm'))) return;
		try {
			await api.clearDownloads();
			await loadDownloads();
		} catch (e) {
			toast.error(String(e));
		}
	}

	async function playCollection(collection: api.DownloadCollection) {
		await goto(`/downloads/${encodeURIComponent(collection.id)}`);
	}

	async function removeCollection(collection: api.DownloadCollection) {
		if (!confirm(t('downloads.remove_collection_confirm'))) return;
		try {
			await api.removeDownloadCollection(collection.id);
			await loadDownloads();
		} catch (e) {
			toast.error(String(e));
		}
	}
</script>

<div class="flex flex-col gap-6">
	<div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-card p-4">
		<div class="min-w-0">
			<h2 class="font-heading text-lg font-semibold">{t('downloads.title')}</h2>
			<p class="truncate text-xs text-muted-foreground">
				{#if downloads.library?.managedDir}{downloads.library.managedDir}{:else}{t('downloads.no_folder')}{/if}
			</p>
			{#if downloads.library}<p class="mt-1 text-xs text-muted-foreground">{bytes(downloads.library.bytes)}</p>{/if}
		</div>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" onclick={chooseFolder}>{t('downloads.choose_folder')}</Button>
			{#if downloads.library?.items.length}
				<Button variant="destructive" size="sm" onclick={removeAll}>{t('downloads.remove_all')}</Button>
			{/if}
		</div>
	</div>

	{#if downloads.error}
		<p class="text-sm text-destructive">{downloads.error}</p>
	{:else if !downloads.library?.parent}
		<div class="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">{t('downloads.configure_hint')}</div>
	{:else if !downloads.library?.storageAvailable}
		<div class="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">{t('downloads.storage_unavailable')}</div>
	{:else}
		{#if active.length}
			<section>
				<h3 class="mb-2 font-heading text-base font-semibold">{t('downloads.queue')}</h3>
				<div class="divide-y overflow-hidden rounded-xl border bg-card">
					{#each active as item (item.song.video_id)}
						<div class="flex items-center gap-3 p-3">
							<div class="min-w-0 flex-1">
								<p class="truncate text-sm font-medium">{item.song.title}</p>
								<p class="truncate text-xs text-muted-foreground">{item.song.artists} · {item.state}</p>
								<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-muted"><div class="h-full bg-primary transition-all" style={`width:${percent(item)}%`}></div></div>
							</div>
							<span class="w-12 text-right text-xs text-muted-foreground">{percent(item)}%</span>
							{#if item.state === 'downloading' || item.state === 'resolving'}
								<Button variant="ghost" size="sm" onclick={() => action(item, 'pause')}>{t('downloads.pause')}</Button>
							{:else if item.state === 'paused'}
								<Button variant="ghost" size="sm" onclick={() => action(item, 'resume')}>{t('downloads.resume')}</Button>
							{:else if item.state === 'failed'}
								<Button variant="ghost" size="sm" onclick={() => action(item, 'retry')}>{t('downloads.retry')}</Button>
							{/if}
							<Button variant="ghost" size="sm" onclick={() => action(item, 'cancel')}>{t('common.cancel')}</Button>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		{#if downloads.library?.collections.length}
			<section>
				<h3 class="mb-2 font-heading text-base font-semibold">{t('downloads.collections')}</h3>
				<div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
					{#each downloads.library.collections as collection (collection.id)}
						<div class="rounded-xl border bg-card p-3 transition-colors hover:bg-accent/10">
							<button class="w-full text-left" onclick={() => playCollection(collection)}>
								<p class="truncate font-medium">{collection.title}</p>
								<p class="mt-1 text-xs text-muted-foreground">{collection.items.length} · {collection.state}</p>
							</button>
							<Button variant="ghost" size="sm" class="mt-2" onclick={() => removeCollection(collection)}>{t('downloads.remove')}</Button>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<section>
			<h3 class="mb-2 font-heading text-base font-semibold">{t('downloads.songs')}</h3>
			{#if complete.length}
				<div class="divide-y overflow-hidden rounded-xl border bg-card">
					{#each complete as item (item.song.video_id)}
						<div class="flex items-center gap-3 p-3">
							<button class="min-w-0 flex-1 text-left" onclick={() => playSong(item.song)}>
								<p class="truncate text-sm font-medium">{item.song.title}</p>
								<p class="truncate text-xs text-muted-foreground">{item.song.artists} · {bytes(item.sizeBytes)}</p>
							</button>
							<Button variant="ghost" size="sm" onclick={() => action(item, 'remove')}>{t('downloads.remove')}</Button>
						</div>
					{/each}
				</div>
			{:else if !active.length && !downloads.library?.collections.length}
				<p class="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">{t('downloads.empty')}</p>
			{/if}
		</section>
	{/if}
</div>
