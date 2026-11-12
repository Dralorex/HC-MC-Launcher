<script setup lang="ts">
import { Button, defineMessages, useVIntl } from '@modrinth/ui'
import { onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'

const emit = defineEmits<{
	skip: []
}>()

const router = useRouter()
const { formatMessage } = useVIntl()

const messages = defineMessages({
	welcomeTitle: {
		id: 'app.welcome-screen.title',
		defaultMessage: 'Welcome to The Hollow Crown Launcher',
	},
	welcomeDescription: {
		id: 'app.welcome-screen.description',
		defaultMessage: 'Joining for an Event or using our Modpacks?',
	},
	openModpacks: {
		id: 'app.welcome-screen.open-modpacks',
		defaultMessage: 'Open Modpacks',
	},
})

const offline = ref(!navigator.onLine)

function handleOffline() {
	offline.value = true
}

function handleOnline() {
	offline.value = false
}

async function openModpacks() {
	emit('skip')
	await router.push('/catalog')
}

onMounted(() => {
	window.addEventListener('offline', handleOffline)
	window.addEventListener('online', handleOnline)
})

onUnmounted(() => {
	window.removeEventListener('offline', handleOffline)
	window.removeEventListener('online', handleOnline)
})
</script>

<template>
	<div class="flex flex-col min-h-full px-6 pb-6 pt-16">
		<div class="relative flex grow items-center justify-center">
			<div class="relative isolate flex flex-col items-center gap-6">
				<div
					class="dot-pattern pointer-events-none absolute left-1/2 -top-52 -z-10 h-[29.875rem] w-[min(25.9375rem,80vw)] -translate-x-1/2 rounded-2xl [@media(max-height:700px)]:h-[23rem]"
					aria-hidden="true"
				/>
				<div class="flex flex-col items-center gap-2">
					<h1 class="m-0 flex items-center gap-2 text-2xl font-semibold leading-8 text-contrast">
						{{ formatMessage(messages.welcomeTitle) }}
					</h1>
					<p class="m-0 text-center text-base leading-6 text-primary">
						{{ formatMessage(messages.welcomeDescription) }}
					</p>
				</div>
				<div class="flex w-72 flex-col items-center gap-4">
					<Button
						type="colored"
						color="brand"
						size="lg"
						class="!shadow-none w-full justify-center"
						:disabled="offline"
						@click="openModpacks"
					>
						{{ formatMessage(messages.openModpacks) }}
					</Button>
				</div>
			</div>
		</div>
	</div>
</template>

<style scoped>
.dot-pattern {
	background-image: radial-gradient(
		circle,
		color-mix(in srgb, var(--color-text-primary) 25%, transparent) 0.5px,
		transparent 0.75px
	);
	background-size: 0.5625rem 0.5625rem;
	opacity: 0.8;
	-webkit-mask-image: radial-gradient(ellipse at center, black 10%, transparent 68%);
	mask-image: radial-gradient(ellipse at center, black 10%, transparent 68%);
	-webkit-mask-repeat: no-repeat;
	mask-repeat: no-repeat;
}
</style>
