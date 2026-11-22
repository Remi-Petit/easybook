<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { formatBytes, formatDate, readUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import {
		offline,
		reloadOffline,
		refreshUsage,
		removeAllOffline,
		removeOffline,
		requestPersistence
	} from '$lib/offline/store.svelte';
	import { flushNow, sync } from '$lib/offline/sync.svelte';
	import { install, promptInstall } from '$lib/install.svelte';

	let confirmDialog = $state<{ show: () => Promise<void> }>();

	onMount(() => {
		void reloadOffline();
		void refreshUsage();
	});

	/** Part du quota occupée par l'origine (bornée pour la barre). */
	const usedPercent = $derived(
		offline.quota > 0 ? Math.min(100, Math.round((offline.usage / offline.quota) * 100)) : 0
	);

	const jobs = $derived(Object.values(offline.jobs));

	async function protect(): Promise<void> {
		await requestPersistence();
	}

	async function removeAll(): Promise<void> {
		await removeAllOffline();
	}
</script>

<svelte:head>
	<title>{t('offline.title')} · easybook</title>
</svelte:head>

<div class="page offline-page">
	<section class="intro">
		<h1>{t('offline.title')}</h1>
		<p>{t('offline.intro')}</p>
	</section>

	{#if !offline.supported}
		<p class="offline-note">{t('download.error.unsupported')}</p>
	{:else}
		<!-- Espace et installation : deux choses que l'on veut savoir d'un coup d'œil. -->
		<section class="offline-panel">
			<div class="offline-kpi">
				<span class="offline-label">{t('offline.storage')}</span>
				<strong>{formatBytes(offline.usage)}</strong>
				{#if offline.quota > 0}
					<span class="offline-sub">{t('offline.usage', {
						used: formatBytes(offline.usage),
						quota: formatBytes(offline.quota)
					})}</span>
					<span class="storage-bar"><span style:width="{usedPercent}%"></span></span>
				{/if}
			</div>

			<div class="offline-kpi">
				<span class="offline-label">{t('offline.title')}</span>
				<strong>{t('offline.books', { count: offline.items.length })}</strong>
				{#if !offline.persisted && offline.items.length > 0}
					<button class="btn-plain offline-protect" type="button" onclick={protect}>
						{t('offline.protect')}
					</button>
					<span class="offline-sub">{t('offline.protectHint')}</span>
				{:else if offline.persisted}
					<span class="offline-sub">{t('offline.protected')}</span>
				{/if}
			</div>

			<div class="offline-kpi">
				<span class="offline-label">easybook</span>
				{#if install.installed}
					<span class="offline-sub">{t('offline.installed')}</span>
				{:else if install.available}
					<button class="btn-plain offline-install" type="button" onclick={() => promptInstall()}>
						{t('offline.install')}
					</button>
					<span class="offline-sub">{t('offline.installHint')}</span>
				{:else}
					<span class="offline-sub">{t('offline.installHint')}</span>
				{/if}
			</div>
		</section>

		{#if jobs.length > 0}
			<section class="offline-jobs" aria-live="polite">
				{#each jobs as job (job.label)}
					<div class="offline-job">
						<span class="offline-job-title">{job.label}</span>
						<span class="storage-bar"><span style:width="{job.total > 0 ? Math.round((job.done / job.total) * 100) : 0}%"></span></span>
						<span class="offline-job-count">{job.done} / {job.total}</span>
					</div>
				{/each}
			</section>
		{/if}

		{#if !sync.online && sync.pending > 0}
			<p class="offline-note">{t('offline.pending', { count: sync.pending })}</p>
		{:else if sync.online && sync.pending > 0}
			<button class="btn-plain offline-sync" type="button" onclick={() => flushNow()}>
				{t('offline.pending', { count: sync.pending })}
			</button>
		{/if}

		{#if offline.items.length === 0}
			<div class="offline-empty">
				<p>{t('offline.empty')}</p>
				<a class="btn-plain" href="/">{t('offline.emptyLink')}</a>
			</div>
		{:else}
			<div class="offline-toolbar">
				<span>{t('offline.books', { count: offline.items.length })}</span>
				<button class="btn-plain" type="button" onclick={() => confirmDialog?.show()}>
					{t('offline.removeAll')}
				</button>
			</div>

			<ul class="offline-list">
				{#each offline.items as item (item.rel)}
					<li class="offline-row">
						<a class="offline-row-main" href={readUrl(item.rel)} data-sveltekit-reload>
							<span class="offline-title">{item.title}</span>
							<span class="offline-meta">
								{item.format.toUpperCase()}
								{#if item.pages}· {t(item.format === 'epub' ? 'card.chapters' : 'card.pages', { count: item.pages })}{/if}
								· {formatBytes(item.bytes || item.size)}
								{#if item.downloadedAt}· {t('offline.added', { date: formatDate(item.downloadedAt) })}{/if}
							</span>
						</a>
						<button
							class="btn-plain offline-remove"
							type="button"
							onclick={() => removeOffline(item.rel)}
						>
							{t('offline.remove')}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</div>

<ConfirmDialog
	bind:this={confirmDialog}
	title={t('offline.removeAllTitle')}
	message={t('offline.removeAllBody')}
	confirmLabel={t('offline.removeAll')}
	onconfirm={removeAll}
/>
