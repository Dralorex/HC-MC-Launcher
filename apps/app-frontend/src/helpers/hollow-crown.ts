import { invoke } from '@tauri-apps/api/core'

import type { InstallJobSnapshot } from './install'

export async function hollow_crown_install_pack(name: string) {
	return await invoke<InstallJobSnapshot>('plugin:hollow-crown|hollow_crown_install_pack', {
		name,
	})
}

/** Starts a repair from the catalog `.mrpack`. `null` means the instance still uses Modrinth ids. */
export async function hollow_crown_repair_instance(instanceId: string) {
	return await invoke<InstallJobSnapshot | null>(
		'plugin:hollow-crown|hollow_crown_repair_instance',
		{ instanceId },
	)
}
