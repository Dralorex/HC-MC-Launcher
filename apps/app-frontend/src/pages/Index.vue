<script setup lang="ts">
import { PlayIcon } from '@modrinth/assets'
import {
	defineMessages,
	injectAuth,
	injectModrinthClient,
	injectNotificationManager,
	useVIntl,
} from '@modrinth/ui'
import { useQuery, useQueryClient } from '@tanstack/vue-query'
import dayjs from 'dayjs'
import { computed, watch } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'

import LibrarySection from '@/components/ui/library/index.vue'
import { libraryScrollTop } from '@/components/ui/library/view-state'
import WelcomeScreen from '@/components/ui/WelcomeScreen.vue'
import RecentWorldsList from '@/components/ui/world/RecentWorldsList.vue'
import { useAppSettings } from '@/composables/use-app-settings.ts'
import { useHiddenInstances } from '@/features/library/hidden-instances/use-hidden-instances'
import { prefetchSharedInstanceLibrary } from '@/features/library/shared-instances/queries'
import { traceStartupStep } from '@/helpers/startup-debug'
import { instanceListQueryOptions } from '@/pages/instance/query-options'
import { skipCreateInstanceOnboarding } from '@/platform/app-lib/onboarding-checklist/commands'
import { injectOnboardingChecklist } from '@/providers/onboarding-checklist'
import { useRootBreadcrumb } from '@/shared/breadcrumbs'

defineOptions({
	name: 'LibraryPage',
})

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { hasCreatedInstance, isReady } = injectOnboardingChecklist()
const appSettings = useAppSettings()
const { hiddenIds: hiddenInstanceIds } = useHiddenInstances()
const auth = injectAuth()
const modrinthClient = injectModrinthClient()
const queryClient = useQueryClient()
onBeforeRouteLeave(() => {
	libraryScrollTop.value = document.querySelector('.app-viewport')?.scrollTop ?? 0
})

const messages = defineMessages({
	home: {
		id: 'app.navigation.home',
		defaultMessage: 'Home',
	},
})

useRootBreadcrumb({
	slot: 'root',
	id: 'home',
	label: formatMessage(messages.home),
	to: '/library',
	visual: { type: 'icon', component: PlayIcon },
})

watch(
	() => (isReady.value && !hasCreatedInstance.value ? auth.user.value?.id : null),
	(userId) => {
		if (!userId) return
		prefetchSharedInstanceLibrary(queryClient, modrinthClient, userId).catch((error) =>
			console.warn('Failed to prefetch shared instance library:', error),
		)
	},
	{ immediate: true },
)

function skipWelcomeScreen() {
	skipCreateInstanceOnboarding().catch(handleError)
}

const instancesQuery = useQuery(instanceListQueryOptions())
const instances = computed(() => instancesQuery.data.value ?? [])
if (hasCreatedInstance.value) {
	await traceStartupStep('Load library instances', () => instancesQuery.suspense()).catch(
		handleError,
	)
}

const recentInstances = computed(() =>
	instances.value
		.filter((instance) => !instance.launch_blocked && !hiddenInstanceIds.value.has(instance.id))
		.sort((a, b) => dayjs(b.last_played ?? b.created).diff(dayjs(a.last_played ?? a.created))),
)
</script>

<template>
	<WelcomeScreen v-if="isReady && !hasCreatedInstance" @skip="skipWelcomeScreen" />
	<div v-else-if="isReady" data-library-page-background class="flex flex-col gap-3 p-6">
		<RecentWorldsList
			v-if="recentInstances?.length > 0 && appSettings.showJumpIn"
			:recent-instances="recentInstances"
		/>
		<LibrarySection :instances="instances" />
	</div>
</template>
