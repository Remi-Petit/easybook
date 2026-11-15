<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import { dismissToast, toasts } from '$lib/toasts.svelte';
</script>

<!-- `aria-live` : un lecteur d'écran annonce le message sans voler le focus. -->
<div class="toasts" role="status" aria-live="polite">
	{#each toasts.items as toast (toast.id)}
		<div class="toast" data-tone={toast.tone}>
			<div class="toast-head">
				<span class="toast-text">{toast.title}</span>
				<button
					class="toast-close"
					type="button"
					aria-label={t('common.dismiss')}
					onclick={() => dismissToast(toast.id)}
				>
					×
				</button>
			</div>
			{#if toast.detail}
				<span class="toast-detail">{toast.detail}</span>
			{/if}
			<!-- La barre *est* la minuterie : elle se vide en `life`, et sa fin
			     retire le message. -->
			<div class="toast-bar">
				<span
					style:animation-duration="{toast.life}ms"
					onanimationend={() => dismissToast(toast.id)}
				></span>
			</div>
		</div>
	{/each}
</div>
