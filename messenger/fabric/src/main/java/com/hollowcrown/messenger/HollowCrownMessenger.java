package com.hollowcrown.messenger;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;

/**
 * On join, reads the token file and sends token id, expiry, signature, and the
 * mod list on {@code hollowcrown:eventgate}. No key, no lists, no staff checks.
 */
public final class HollowCrownMessenger implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        PayloadTypeRegistry.playC2S().register(VerifyPayload.ID, VerifyPayload.CODEC);
        ClientPlayConnectionEvents.JOIN.register((handler, sender, client) -> client.execute(() -> {
            VerifyPayload payload = VerifyPayload.fromGameDirectory();
            ClientPlayNetworking.send(payload);
        }));
    }
}
