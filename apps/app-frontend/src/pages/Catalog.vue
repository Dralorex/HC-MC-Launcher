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
const installHints = ref({})

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

function formatInstallError(error, pack) {
	const raw = String(error?.message || error || '')
	const lower = raw.toLowerCase()
	if (
		lower.includes('404') ||
		lower.includes('not found') ||
		lower.includes('does not exist') ||
		lower.includes('unknown project') ||
		lower.includes('project not found') ||
		lower.includes('version not found')
	) {
		return `This pack is listed in the Hollow Crown catalog, but Modrinth returned 404 for project “${pack.modrinth_project_id}” / version “${pack.modrinth_version_id}”. Publish or un-private that Modrinth pack (or update the catalog ids), then try Install again.`
	}
	return raw || 'Install failed.'
}

async function installPack(pack) {
	installingId.value = pack.modrinth_project_id
	installHints.value = { ...installHints.value, [pack.modrinth_project_id]: '' }
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
		const message = formatInstallError(error, pack)
		installHints.value = { ...installHints.value, [pack.modrinth_project_id]: message }
		handleError(new Error(message))
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
			class="bg-bg-raised rounded-xl p-4 flex flex-col gap-3"
		>
			<div class="flex items-center justify-between gap-4">
				<div>
					<h2 class="text-lg font-semibold text-contrast m-0">{{ pack.name }}</h2>
					<p v-if="pack.summary" class="text-secondary mt-1 mb-0">{{ pack.summary }}</p>
					<p class="text-secondary mt-1 mb-0 text-sm">play.thehollowcrownmc.com</p>
				</div>
				<button
					class="btn btn-primary shrink-0"
					:disabled="installingId === pack.modrinth_project_id"
					@click="installPack(pack)"
				>
					{{ installingId === pack.modrinth_project_id ? 'Installing…' : 'Install' }}
				</button>
			</div>
			<p
				v-if="installHints[pack.modrinth_project_id]"
				class="text-sm text-secondary m-0"
			>
				{{ installHints[pack.modrinth_project_id] }}
			</p>
		</article>
	</div>
</template>
