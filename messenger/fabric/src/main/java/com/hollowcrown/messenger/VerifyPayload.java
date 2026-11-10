package com.hollowcrown.messenger;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.stream.Stream;
import net.minecraft.network.PacketByteBuf;
import net.minecraft.network.codec.PacketCodec;
import net.minecraft.network.packet.CustomPayload;
import net.minecraft.util.Identifier;

/**
 * Plugin message {@code hollowcrown:eventgate}.
 *
 * The body is UTF-8 JSON with {@code token_id}, {@code expiry}, {@code signature},
 * and {@code mods}. Those three token fields are copied from the join-token file.
 * {@code signature} is the server's Ed25519 signature. This mod does not hold a key
 * and does not check the signature.
 */
public record VerifyPayload(byte[] body) implements CustomPayload {
    public static final Id<VerifyPayload> ID = new Id<>(Identifier.of("hollowcrown", "eventgate"));
    public static final PacketCodec<PacketByteBuf, VerifyPayload> CODEC = PacketCodec.of(
        (value, buf) -> buf.writeBytes(value.body),
        buf -> {
            byte[] body = new byte[buf.readableBytes()];
            buf.readBytes(body);
            return new VerifyPayload(body);
        }
    );

    @Override
    public Id<? extends CustomPayload> getId() {
        return ID;
    }

    public static VerifyPayload fromGameDirectory() {
        Path root = Path.of("").toAbsolutePath();
        byte[] tokenBytes = readToken(root.resolve("hollow-crown").resolve("join-token.json"));
        JsonObject message = new JsonObject();
        try {
            JsonObject token = com.google.gson.JsonParser.parseString(
                new String(tokenBytes, StandardCharsets.UTF_8)
            ).getAsJsonObject();
            copyField(message, token, "token_id");
            copyField(message, token, "expiry");
            copyField(message, token, "signature");
        } catch (RuntimeException ignored) {
            // The server rejects a message that is missing the token fields.
        }
        message.add("mods", modEntries(root));
        return new VerifyPayload(message.toString().getBytes(StandardCharsets.UTF_8));
    }

    private static void copyField(JsonObject message, JsonObject token, String name) {
        if (token.has(name)) {
            message.add(name, token.get(name));
        }
    }

    private static byte[] readToken(Path path) {
        try {
            if (Files.isRegularFile(path)) {
                return Files.readAllBytes(path);
            }
        } catch (Exception ignored) {
            // A missing or unreadable token is sent as empty bytes. The server rejects it.
        }
        return new byte[0];
    }

    private static JsonArray modEntries(Path root) {
        JsonArray mods = new JsonArray();
        for (String folder : List.of("mods", "resourcepacks", "shaderpacks")) {
            Path dir = root.resolve(folder);
            if (!Files.isDirectory(dir)) {
                continue;
            }
            List<Path> files = new ArrayList<>();
            try (Stream<Path> stream = Files.list(dir)) {
                stream.filter(Files::isRegularFile).filter(VerifyPayload::isArchive).forEach(files::add);
            } catch (Exception ignored) {
                continue;
            }
            files.sort(Comparator.comparing(path -> path.getFileName().toString()));
            for (Path file : files) {
                try {
                    byte[] bytes = readAll(file);
                    JsonObject entry = new JsonObject();
                    entry.addProperty("path", folder + "/" + file.getFileName());
                    entry.addProperty("sha1", hex(digest("SHA-1", bytes)));
                    entry.addProperty("sha512", hex(digest("SHA-512", bytes)));
                    entry.addProperty("size", bytes.length);
                    mods.add(entry);
                } catch (Exception ignored) {
                    JsonObject entry = new JsonObject();
                    entry.addProperty("path", folder + "/" + file.getFileName());
                    entry.addProperty("unreadable", true);
                    mods.add(entry);
                }
            }
        }
        return mods;
    }

    private static boolean isArchive(Path path) {
        String name = path.getFileName().toString().toLowerCase();
        return name.endsWith(".jar") || name.endsWith(".zip") || name.endsWith(".litemod");
    }

    private static byte[] readAll(Path path) throws Exception {
        try (InputStream input = Files.newInputStream(path)) {
            return input.readAllBytes();
        }
    }

    private static byte[] digest(String algorithm, byte[] bytes) throws Exception {
        return MessageDigest.getInstance(algorithm).digest(bytes);
    }

    private static String hex(byte[] bytes) {
        StringBuilder builder = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            builder.append(String.format("%02x", value));
        }
        return builder.toString();
    }
}
