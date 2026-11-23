<script lang="ts">
	import { onMount } from 'svelte';
	import { goto, invalidateAll, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { formatDate, type BookInfo } from '$lib/api';
	import {
		call,
		displayName,
		errorKey,
		oidcLinkUrl,
		type BookProgress,
		type ReadingHistory,
		type Session
	} from '$lib/auth';
	import { previewBook } from '$lib/catalog';
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import PasswordField from '$lib/components/PasswordField.svelte';
	import Rail from '$lib/components/Rail.svelte';
	import { formatNumber, formatReadingDuration, intlLocale, t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { registerPasskey, passkeySupported } from '$lib/passkeys';
	import { pins, startPins, unpinCompleted } from '$lib/pins.svelte';
	import {
		completedSeries,
		isFinished,
		readingBuckets,
		readingYears,
		watchLaterEntries
	} from '$lib/progress';
	import { notify } from '$lib/toasts.svelte';

	let { data }: { data: { session: Session } } = $props();

	const user = $derived(data.session.user);
	const oidc = $derived(data.session.oidc);
	const canLink = $derived(!!oidc && (oidc.self_link || user?.role === 'admin'));

	let current = $state('');
	let fresh = $state('');
	let busy = $state(false);
	let passkeyName = $state('');
	let passkeyPassword = $state('');
	let passkeyBusy = $state(false);
	/** WebAuthn dépend du navigateur : `null` tant que l'hydratation n'a pas décidé. */
	let canPasskey = $state<boolean | null>(null);

	// Onglets du compte : chacun a son adresse (`/account/collection`, `/reading`,
	// `/password`, `/sso`). Un onglet n'existe que si le serveur le propose : SSO
	// avec un fournisseur. Les passkeys vivent dans l'onglet « Mot de passe » : ce
	// sont deux façons de se connecter, pas deux sujets.
	const labels = {
		collection: 'account.tab.collection',
		reading: 'account.tab.reading',
		password: 'account.password.tab',
		sso: 'users.type.sso'
	} as const;
	type Tab = keyof typeof labels;
	const tabs = $derived<Tab[]>([
		'collection',
		'reading',
		'password',
		...(oidc ? (['sso'] as const) : [])
	]);
	/** Onglet demandé par l'URL. L'ancienne adresse `/account/passkeys`, dont des
	 *  liens peuvent encore exister, ouvre « Mot de passe » ; `/account` sans
	 *  suffixe ouvre la collection. */
	const asked = $derived(page.params.tab === 'passkeys' ? 'password' : page.params.tab);
	const tab = $derived<Tab>(tabs.includes(asked as Tab) ? (asked as Tab) : 'collection');
	const tabLabel = (id: Tab) => t(labels[id]);

	async function tabKeys(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		event.preventDefault();
		const next = tabs[(tabs.indexOf(tab) + step + tabs.length) % tabs.length]!;
		// Le focus suit l'onglet : on attend la navigation, sinon le lien est
		// encore l'ancien au moment de le focaliser.
		await goto(`/account/${next}`);
		document.getElementById(`account-tab-${next}`)?.focus();
	}

	// Période de lecture : le mois en cours, l'année en cours, ou tout — elle a
	// aussi son adresse (`/account/reading/year`) ; `/account/reading` seul ouvre
	// le mois en cours.
	const periods = ['month', 'year', 'all'] as const;
	type Period = (typeof periods)[number];
	const period = $derived<Period>(
		periods.includes(page.params.period as Period) ? (page.params.period as Period) : 'month'
	);
	const periodLabel = (id: Period) => t(`account.reading.period.${id}`);

	/** Premier instant de la période, en secondes Unix (`0` pour « tout »). */
	function periodStart(id: Period): number {
		if (id === 'all') return 0;
		const today = new Date();
		const start =
			id === 'month'
				? new Date(today.getFullYear(), today.getMonth(), 1)
				: new Date(today.getFullYear(), 0, 1);
		return Math.floor(start.getTime() / 1000);
	}

	// Les livres terminés : toute la liste du compte (`limit=0` — le serveur ne
	// plafonne plus, sinon les séries terminées seraient sous-estimées). Elle
	// sert aux trois périodes, qui ne sont que des filtres sur `updatedAt`.
	let finished = $state<BookProgress[]>([]);
	// Les chiffres d'une période viennent du serveur : lui seul connaît le temps
	// de lecture (les sessions ne sont pas toutes envoyées au client).
	const summaries = $state<Partial<Record<Period, ReadingHistory['summary']>>>({});
	const loaded = $state<Record<Period, boolean>>({ month: false, year: false, all: false });
	/** Périodes en cours de chargement : un second appel ne repart pas. */
	const pending = new Set<Period>();

	async function loadPeriod(id: Period) {
		if (loaded[id] || pending.has(id)) return;
		pending.add(id);
		const since = periodStart(id);
		// Seul « tout » a besoin de la liste entière ; pour le mois et l'année, seuls
		// les chiffres servent — une ligne suffit à remplir le résumé.
		const limit = id === 'all' ? 0 : 1;
		const result = await call<ReadingHistory>(
			'GET',
			`/api/read/history?limit=${limit}&since=${since}`
		);
		if (result.ok) {
			summaries[id] = result.data.summary;
			if (id === 'all') {
				finished = result.data.recent.filter((entry) => isFinished(entry.page, entry.pages));
			}
		}
		loaded[id] = true;
		pending.delete(id);
	}

	async function periodKeys(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		event.preventDefault();
		const next = periods[(periods.indexOf(period) + step + periods.length) % periods.length]!;
		// Le focus suit l'onglet (voir `tabKeys`).
		await goto(`/account/reading/${next}`);
		document.getElementById(`account-period-${next}`)?.focus();
	}

	onMount(async () => {
		canPasskey = passkeySupported();
		// Retour d'une liaison OIDC : `?linked=1` ou `?error=<code>`.
		const params = page.url.searchParams;
		if (params.get('linked')) notify(t('account.linked'), { tone: 'ok' });
		else if (params.get('error')) notify(t(errorKey(params.get('error'))), { tone: 'error' });
		if (params.get('linked') || params.get('error')) replaceState(`/account/${tab}`, {});

		// Les épingles du compte : la collection ne dépend pas du catalogue.
		startPins();
		// La liste entière (livres terminés, séries) : la collection en vit aussi ;
		// les chiffres de la période affichée sont chargés par l'effet ci-dessous.
		await loadPeriod('all');
	});

	// La période vient de l'URL : ses chiffres sont demandés au serveur — une
	// fois par période, `loaded` garde le résultat quand on revient dessus.
	$effect(() => {
		if (tab !== 'reading') return;
		void loadPeriod(period);
	});

	/** Début de la période affichée : la même borne pour les cartes et la liste. */
	const since = $derived(periodStart(period));
	const summary = $derived(summaries[period] ?? null);

	/** Séries terminées : les dossiers dont tous les livres sont finis, et dont le
	 *  dernier tome a été terminé dans la période. */
	const series = $derived(completedSeries(library.books, finished, since));

	/** Les épingles « à regarder plus tard » : encore au catalogue, pas encore lues. */
	const collection = $derived(
		watchLaterEntries(
			pins.items.map((pin) => pin.item),
			library.folders,
			library.books,
			finished
		)
	);

	// Une épingle dont le livre — ou la série — est entièrement lu n'a plus rien
	// à regarder : elle est détachée, ici comme sur l'accueil.
	$effect(() => {
		if (!loaded.all) return;
		void unpinCompleted(library.books, finished);
	});

	const monthFormat = $derived(
		new Intl.DateTimeFormat(intlLocale(), { month: 'long', year: 'numeric' })
	);
	const monthName = $derived(new Intl.DateTimeFormat(intlLocale(), { month: 'long' }));
	const yearFormat = $derived(new Intl.DateTimeFormat(intlLocale(), { year: 'numeric' }));

	/** Le dossier d'une série, s'il est encore au catalogue. */
	const folderOf = (rel: string) => library.folders.find((entry) => entry.rel === rel);

	/**
	 * Les livres terminés des onglets « Mois » et « Année » : une seule tranche
	 * (sans titre) pour le mois, et mois par mois pour l'année.
	 */
	const groups = $derived(
		period === 'all'
			? []
			: readingBuckets(library.books, finished, period, since).map((bucket) => ({
					...bucket,
					label:
						bucket.key === 'month' ? null : monthFormat.format(new Date(bucket.at * 1000))
				}))
	);

	/** Onglet « Tout » : les années, chacune avec ses mois. */
	const years = $derived(
		period === 'all' ? readingYears(library.books, finished, since) : []
	);

	/** « 2 h 05 », « 47 min », « 12 s » : une durée se lit d'un coup d'œil. */
	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		const result = await call('POST', '/api/auth/password', { current, new: fresh });
		busy = false;
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		current = fresh = '';
		notify(t('account.password.changed'), { tone: 'ok' });
		await invalidateAll();
	}

	async function unlink(id: string) {
		const result = await call('DELETE', `/api/auth/identities/${id}`);
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		notify(t('account.unlinked'), { tone: 'ok' });
		await invalidateAll();
	}

	/** Crée une passkey : le navigateur demande le code ou la biométrie. */
	async function addPasskey(event: SubmitEvent) {
		event.preventDefault();
		if (passkeyBusy) return;
		passkeyBusy = true;
		const result = await registerPasskey(passkeyName.trim(), passkeyPassword);
		passkeyBusy = false;
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		passkeyName = '';
		passkeyPassword = '';
		notify(t('account.passkeys.added'), { tone: 'ok' });
		await invalidateAll();
	}

	async function removePasskey(id: string) {
		const result = await call('DELETE', `/api/auth/passkey/${id}`);
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		notify(t('account.passkeys.removed'), { tone: 'ok' });
		await invalidateAll();
	}
</script>

<svelte:head>
	<title>{t('account.title')} · easybook</title>
</svelte:head>

<!-- Les cartes d'une tranche : ses séries achevées, puis ses livres. -->
{#snippet readingCards(books: BookInfo[], series: string[])}
	{#each series as rel (rel)}
		{@const folder = folderOf(rel)}
		{#if folder}
			<FolderCard {folder} preview={previewBook(library.folders, library.books, rel)} />
		{/if}
	{/each}
	{#each books as book (book.rel)}
		<BookCard {book} />
	{/each}
{/snippet}

<div class="page">
	<section class="intro">
		<h1>{t('account.title')}</h1>
		{#if user}
			<p>
				{displayName(user)} · <code>{user.email}</code> · {t(`account.role.${user.role}`)}
			</p>
		{/if}
	</section>

	{#if user}
		<div class="tabs" role="tablist" tabindex="-1" onkeydown={tabKeys}>
			{#each tabs as id (id)}
				<a
					role="tab"
					id="account-tab-{id}"
					href="/account/{id}"
					aria-selected={tab === id}
					aria-controls="account-panel"
					tabindex={tab === id ? 0 : -1}
				>
					{tabLabel(id)}
				</a>
			{/each}
		</div>

		<div id="account-panel" class="tab-panel" role="tabpanel" aria-labelledby="account-tab-{tab}">
			<div class="admin-stack">
				{#if tab === 'collection'}
					<section class="panel-card">
						<h2>
							{t('account.collection.title')}
							<span class="perf-count">{collection.length}</span>
						</h2>
						{#if !pins.loaded}
							<p class="panel-hint">{t('status.connecting')}</p>
						{:else if collection.length === 0}
							<p class="panel-hint">{t('account.collection.empty')}</p>
						{:else}
							<div class="grid">
								{#each collection as entry (entry.rel)}
									{#if entry.folder}
										<FolderCard
											folder={entry.folder}
											preview={previewBook(library.folders, library.books, entry.rel)}
										/>
									{:else if entry.book}
										<BookCard book={entry.book} />
									{/if}
								{/each}
							</div>
						{/if}
					</section>
				{:else if tab === 'reading'}
					<div class="tabs sub" role="tablist" tabindex="-1" onkeydown={periodKeys}>
						{#each periods as id (id)}
							<a
								role="tab"
								id="account-period-{id}"
								href="/account/reading/{id}"
								aria-selected={period === id}
								aria-controls="account-reading"
								tabindex={period === id ? 0 : -1}
							>
								{periodLabel(id)}
							</a>
						{/each}
					</div>

					<div
						id="account-reading"
						class="admin-stack"
						role="tabpanel"
						aria-labelledby="account-period-{period}"
					>
						<div class="perf-grid compact">
							<section class="perf-card kpi" style="--kpi: #e0a54a">
								<h2>{t('account.reading.kpi.books')}</h2>
								<p class="stat-value">{formatNumber(summary?.finished ?? 0)}</p>
							</section>
							<section class="perf-card kpi" style="--kpi: #6aa5e0">
								<h2>{t('account.reading.kpi.time')}</h2>
								<p class="stat-value">{formatReadingDuration(summary?.seconds ?? 0)}</p>
							</section>
							<section class="perf-card kpi" style="--kpi: #5cc98a">
								<h2>{t('account.reading.kpi.series')}</h2>
								<p class="stat-value">
									{formatNumber(series.done)}
									<span class="stat-of">/ {formatNumber(series.total)}</span>
								</p>
							</section>
						</div>

						{#if !loaded.all}
							<p class="panel-hint">{t('status.connecting')}</p>
						{:else if period === 'all'}
							{#if years.length === 0}
								<p class="panel-hint">{t('account.reading.empty')}</p>
							{:else}
								<!-- « Tout » : chaque année, avec le total de l'année, puis ses mois. -->
								{#each years as year (year.key)}
									{@const label = yearFormat.format(new Date(year.at * 1000))}
									<section class="group">
										<h2>
											{label}
											<span class="count">{year.books.length}</span>
											{#if year.series.length > 0}
												<span class="group-series">
													{t('account.reading.group.series', { count: year.series.length })}
												</span>
											{/if}
										</h2>
										{#each year.months as month (month.key)}
											{@const monthLabel = monthName.format(new Date(month.at * 1000))}
											<h3>
												{monthLabel}
												<span class="count">{month.books.length}</span>
											</h3>
											<Rail label={monthLabel}>
												{@render readingCards(month.books, month.series)}
											</Rail>
										{/each}
									</section>
								{/each}
							{/if}
						{:else if groups.length === 0}
							<p class="panel-hint">{t('account.reading.empty')}</p>
						{:else}
							<!-- Chaque tranche se présente comme une rangée de l'accueil : son
							     titre, son compte, puis ses couvertures au défilement. -->
							{#each groups as group (group.key)}
								{@const label = group.label ?? t('account.reading.title')}
								<section class="group">
									<h2>
										{label}
										<span class="count">{group.books.length}</span>
										{#if group.series.length > 0}
											<span class="group-series">
												{t('account.reading.group.series', { count: group.series.length })}
											</span>
										{/if}
									</h2>
									<Rail {label}>
										{@render readingCards(group.books, group.series)}
									</Rail>
								</section>
							{/each}
						{/if}
					</div>
				{:else if tab === 'password'}
					<section class="panel-card">
						<h2>{t('account.password.title')}</h2>
						{#if !user.has_password}
							<p class="panel-hint">{t('account.password.none')}</p>
						{/if}
						<form class="stack-form" method="post" onsubmit={changePassword}>
							<!-- Champ caché : les gestionnaires de mots de passe retrouvent l'identifiant. -->
							<input
								type="text"
								name="email"
								value={user.email ?? ''}
								autocomplete="username"
								hidden
							/>
							{#if user.has_password}
								<PasswordField
									label={t('account.password.current')}
									bind:value={current}
									autocomplete="current-password"
									generable={false}
									temporaryControls={false}
								/>
							{/if}
							<PasswordField
								label={t('account.password.new')}
								bind:value={fresh}
								temporaryControls={false}
							/>
							<div>
								<button type="submit" class="btn-main" disabled={busy || fresh === ''}>
									{t('account.password.save')}
								</button>
							</div>
						</form>
					</section>
					{#if data.session.passkey}
						<!-- Les passkeys sont une autre façon de se connecter : elles vivent
						     sous le mot de passe, dans le même onglet, pas à part. -->
						<section class="panel-card">
							<h2>{t('account.passkeys.title')}</h2>
							<p class="panel-hint">{t('account.passkeys.intro')}</p>
							{#if canPasskey === null}
								<p class="panel-hint">{t('status.connecting')}</p>
							{:else if !canPasskey}
								<p class="panel-hint">{t('account.passkeys.unsupported')}</p>
							{:else}
								{#if user.passkeys.length > 0}
									<ul class="identity-list">
										{#each user.passkeys as passkey (passkey.id)}
											<li>
												<div>
													<strong>{passkey.name}</strong>
													<span class="muted">
														{t('account.passkeys.created', {
															date: formatDate(passkey.created_at)
														})}
														{#if passkey.last_used_at}
															· {t('account.passkeys.lastUsed', {
																date: formatDate(passkey.last_used_at)
															})}
														{/if}
													</span>
												</div>
												<button
													type="button"
													class="btn-plain"
													onclick={() => removePasskey(passkey.id)}
												>
													{t('account.passkeys.remove')}
												</button>
											</li>
										{/each}
									</ul>
								{:else}
									<p class="panel-hint">{t('account.passkeys.none')}</p>
								{/if}
								<form class="stack-form" method="post" onsubmit={addPasskey}>
									<label class="field">
										<span>{t('account.passkeys.name')}</span>
										<input
											bind:value={passkeyName}
											maxlength="64"
											autocomplete="off"
											placeholder={t('account.passkeys.namePlaceholder')}
										/>
									</label>
									{#if user.has_password}
										<PasswordField
											label={t('account.passkeys.current')}
											bind:value={passkeyPassword}
											autocomplete="current-password"
											generable={false}
											temporaryControls={false}
										/>
									{/if}
									<div>
										<button type="submit" class="btn-main" disabled={passkeyBusy}>
											{passkeyBusy ? t('account.passkeys.adding') : t('account.passkeys.add')}
										</button>
									</div>
								</form>
								<p class="panel-hint">{t('account.passkeys.hint')}</p>
							{/if}
						</section>
					{/if}
				{:else if tab === 'sso' && oidc}
					<section class="panel-card">
						<h2>{t('account.identities.title', { provider: oidc.label })}</h2>
						{#if user.identities.length === 0}
							<p class="panel-hint">{t('account.identities.none')}</p>
						{:else}
							<ul class="identity-list">
								{#each user.identities as identity (identity.id)}
									<li>
										<div>
											<strong>{identity.email ?? identity.subject}</strong>
											<span class="muted">{formatDate(identity.created_at)}</span>
											<code class="identity-sub">{identity.issuer}</code>
										</div>
										<button type="button" class="btn-plain" onclick={() => unlink(identity.id)}>
											{t('account.identities.unlink')}
										</button>
									</li>
								{/each}
							</ul>
						{/if}
						{#if canLink}
							<p>
								<a class="btn-main" href={oidcLinkUrl()}>
									{t('account.identities.link', { provider: oidc.label })}
								</a>
							</p>
						{:else}
							<p class="panel-hint">{t('account.identities.adminOnly')}</p>
						{/if}
					</section>
				{/if}

			</div>
		</div>
	{/if}
</div>
