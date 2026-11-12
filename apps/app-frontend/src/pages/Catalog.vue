<script setup>
import { injectNotificationManager } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import {
	install_create_modpack_instance,
	installJobInstanceId,
	wait_for_install_job,
} from '@/helpers/install'
import { injectAppEvents } from '@/providers/app-events'

const router = useRouter()
const appEvents = injectAppEvents()
const { handleError } = injectNotificationManager()
const packs = ref([])
const loading = ref(true)
const installingId = ref('')
const loadError = ref('')

onMounted(async () => {
	try {
		const catalog = await invoke('plugin:hollow-crown|hollow_crown_catalog')
		packs.value = Array.isArray(catalog?.packs) ? catalog.packs : []
	} catch (error) {
		loadError.value =
			error?.message ||
			'The Hollow Crown Modpacks list could not be loaded. Install stays here once packs are available.'
	} finally {
		loading.value = false
	}
})

async function installPack(pack) {
	installingId.value = pack.modrinth_project_id
	try {
		const job = await install_create_modpack_instance({
			type: 'fromVersionId',
			project_id: pack.modrinth_project_id,
			version_id: pack.modrinth_version_id,
			title: pack.name,
			icon_url: pack.icon_url ?? null,
		})
		const finished = await wait_for_install_job(appEvents, job.job_id)
		const instanceId = installJobInstanceId(finished)
		if (instanceId) {
			await router.push(`/instance/${encodeURIComponent(instanceId)}/`)
		}
	} catch (error) {
		handleError(error)
	} finally {
		installingId.value = ''
	}
}
</script>

<template>
	<div class="p-6 flex flex-col gap-4 max-w-3xl">
		<div>
			<h1 class="text-2xl font-bold text-contrast m-0">The Hollow Crown Modpacks</h1>
			<p class="text-secondary mt-2 mb-0">
				Install, repair, and play Hollow Crown packs. Play connects to
				play.thehollowcrownmc.com. This is not an official Minecraft product.
			</p>
		</div>
		<p v-if="loading" class="text-secondary">Loading modpacks…</p>
		<p v-else-if="loadError" class="text-secondary">{{ loadError }}</p>
		<p v-else-if="packs.length === 0" class="text-secondary">No packs are published yet.</p>
		<article
			v-for="pack in packs"
			:key="pack.modrinth_project_id"
			class="bg-bg-raised rounded-xl p-4 flex items-center justify-between gap-4"
		>
			<div>
				<h2 class="text-lg font-semibold text-contrast m-0">{{ pack.name }}</h2>
				<p v-if="pack.summary" class="text-secondary mt-1 mb-0">{{ pack.summary }}</p>
				<p class="text-secondary mt-1 mb-0 text-sm">play.thehollowcrownmc.com</p>
			</div>
			<button
				class="btn btn-primary"
				:disabled="installingId === pack.modrinth_project_id"
				@click="installPack(pack)"
			>
				{{ installingId === pack.modrinth_project_id ? 'Installing…' : 'Install' }}
			</button>
		</article>
	</div>
</template>
