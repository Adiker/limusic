<script lang="ts">
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { ArrowRight01Icon, Link04Icon } from '@hugeicons/core-free-icons';
	import { Input } from '$lib/components/ui/input';
	import { openLinkTarget } from '$lib/browse';
	import { parseYtLink } from '$lib/ytlink';
	import { toast } from '$lib/player.svelte';
	import { OPEN_LINK_COMBO } from '$lib/shortcuts';
	import { t } from '$lib/i18n.svelte';

	let { large = false, class: className = '' }: { large?: boolean; class?: string } = $props();
	let url = $state('');
	let invalid = $state(false);

	function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!url.trim()) return;
		const target = parseYtLink(url);
		if (!target) {
			invalid = true;
			toast.error(t('dialogs.link.invalid_link'));
			return;
		}
		invalid = false;
		url = '';
		openLinkTarget(target);
	}
</script>

<form class="relative min-w-0 {className}" onsubmit={submit}>
	<HugeiconsIcon
		icon={Link04Icon}
		class="pointer-events-none absolute top-1/2 z-10 -translate-y-1/2 text-muted-foreground {large
			? 'left-4 h-5 w-5'
			: 'left-3 h-4 w-4'}"
	/>
	<Input
		bind:value={url}
		data-link-input
		inputmode="url"
		autocomplete="off"
		spellcheck={false}
		aria-label={t('dialogs.link.title')}
		aria-invalid={invalid}
		placeholder={t('dialogs.link.placeholder')}
		title={t('dialogs.link.desc')}
		class="rounded-full pr-16 {large
			? 'h-12 bg-card/80 pl-12 text-base shadow-sm md:text-base'
			: 'pl-9'}"
		oninput={() => (invalid = false)}
	/>
	{#if url}
		<button
			type="submit"
			disabled={!url.trim()}
			aria-label={t('dialogs.link.open')}
			title={t('dialogs.link.open')}
			class="absolute right-2 top-1/2 z-10 flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
		>
			<HugeiconsIcon icon={ArrowRight01Icon} class="h-4 w-4" />
		</button>
	{:else}
		<kbd
			class="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 rounded border bg-muted px-1.5 py-0.5 font-mono text-[0.625rem] font-medium tracking-wide text-muted-foreground"
		>
			{OPEN_LINK_COMBO}
		</kbd>
	{/if}
</form>
