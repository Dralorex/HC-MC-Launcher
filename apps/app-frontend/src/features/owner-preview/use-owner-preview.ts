import { invoke } from '@tauri-apps/api/core'
import { onMounted, readonly, ref } from 'vue'

const OWNER_EMAIL = 'dralorex@gmail.com'

const ownerPreview = ref(false)
const ready = ref(false)
let loadPromise: Promise<void> | null = null

async function refreshOwnerPreview() {
	try {
		ownerPreview.value = Boolean(
			await invoke<boolean>('plugin:hollow-crown|hollow_crown_owner_preview'),
		)
	} catch {
		ownerPreview.value = false
	} finally {
		ready.value = true
	}
}

export function ensureOwnerPreviewLoaded() {
	if (!loadPromise) {
		loadPromise = refreshOwnerPreview()
	}
	return loadPromise
}

export function useOwnerPreview() {
	onMounted(() => {
		void ensureOwnerPreviewLoaded()
	})

	return {
		ownerPreview: readonly(ownerPreview),
		ownerPreviewReady: readonly(ready),
		ownerEmail: OWNER_EMAIL,
		refreshOwnerPreview,
	}
}
