<script lang="ts">
	import { page } from '$app/state';
	import { afterNavigate } from '$app/navigation';
	import { childFolders, folderUrl } from '$lib/catalog';
	import { library } from '$lib/library.svelte';
	import { LOCALES, chooseLocale, locale, t } from '$lib/i18n/state.svelte';

	// Un dossier de premier niveau de `books/` est une **catégorie**.
	const categories = $derived(childFolders(library.folders, ''));

	const path = $derived(decodeURIComponent(page.url.pathname));

	/** Menu ouvert (écrans étroits : la barre latérale devient un bandeau + un menu). */
	let open = $state(false);

	afterNavigate(() => (open = false));

	/** Une catégorie reste active sur ses sous-dossiers (`/folder/A/B`). */
	function active(href: string): boolean {
		const target = decodeURIComponent(href);
		return path === target || path.startsWith(`${target}/`);
	}
</script>

<svelte:window onkeydown={(event) => event.key === 'Escape' && (open = false)} />

<aside class="sidebar" data-open={open}>
	<div class="sidebar-bar">
		<div class="brand">
			<a class="mark" href="/">easybook</a>
			<span class="tagline">{t('brand.tagline')}</span>
		</div>

		<button
			type="button"
			class="menu-btn"
			aria-expanded={open}
			aria-controls="sidebar-panel"
			aria-label={t('nav.menu')}
			onclick={() => (open = !open)}
		>
			<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true">
				{#if open}
					<path d="M4 4l10 10M14 4L4 14" />
				{:else}
					<path d="M3 5h12M3 9h12M3 13h12" />
				{/if}
			</svg>
		</button>
	</div>

	<div class="sidebar-panel" id="sidebar-panel">
		<nav aria-label={t('nav.label')}>
			<a class="nav-item" href="/" aria-current={path === '/' ? 'page' : undefined}>
				<span class="name">{t('nav.home')}</span>
			</a>

			<p class="nav-section">{t('nav.library')}</p>

			{#if categories.length > 0}
				{#each categories as category (category.rel)}
					<a
						class="nav-item"
						href={folderUrl(category.rel)}
						aria-current={active(folderUrl(category.rel)) ? 'page' : undefined}
						title={category.name}
					>
						<span class="name">{category.name}</span>
						<span class="nav-count">{category.count}</span>
					</a>
				{/each}
			{:else if library.status !== 'connecting'}
				<p class="nav-empty">{t('nav.empty')}</p>
			{/if}
		</nav>

		<div class="sidebar-lang">
			<label for="easybook-locale">{t('language.label')}</label>
			<select
				id="easybook-locale"
				value={locale()}
				title={t('language.choose')}
				onchange={(event) => chooseLocale(event.currentTarget.value)}
			>
				{#each LOCALES as entry (entry.code)}
					<option value={entry.code}>{entry.name}</option>
				{/each}
			</select>
		</div>

		<div class="sidebar-foot">
			<span class="status" data-status={library.status}>
				<span class="dot"></span>
				{#if library.status === 'live'}
					{t('status.live')}
				{:else if library.status === 'connecting'}
					{t('status.connecting')}
				{:else}
					{t('status.offline')}
				{/if}
			</span>
			<span class="nav-count">{library.total}</span>
		</div>
	</div>
</aside>
