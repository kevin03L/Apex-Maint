package fr.apex.maint.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Chaque valeur est chiffrée AES-GCM ; la clé ne quitte jamais Android Keystore. */
class SecureStore(context: Context) {
    private val prefs = context.getSharedPreferences("session_chiffree", Context.MODE_PRIVATE)
    private val alias = "apex_maint_session_v1"
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    @Synchronized fun get(name: String): String? {
        val encoded = prefs.getString(name, null) ?: return null
        return try {
            val bytes = Base64.decode(encoded, Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
            cipher.updateAAD(name.toByteArray())
            cipher.doFinal(bytes.copyOfRange(12, bytes.size)).toString(Charsets.UTF_8)
        } catch (_: Exception) { prefs.edit().remove(name).commit(); null }
    }
    @Synchronized fun put(name: String, value: String?) {
        if (value == null) { prefs.edit().remove(name).commit(); return }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        cipher.updateAAD(name.toByteArray())
        val encoded = Base64.encodeToString(cipher.iv + cipher.doFinal(value.toByteArray()), Base64.NO_WRAP)
        check(prefs.edit().putString(name, encoded).commit()) { "Enregistrement sécurisé impossible" }
    }
    @Synchronized fun clear() { check(prefs.edit().clear().commit()) }
}

class PersistentCookieJar(private val store: SecureStore) : CookieJar {
    // Une seule instance serveur ; cookies de session persistés même sans Max-Age.
    @Synchronized override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        val current = loadForRequest(url).toMutableList()
        cookies.filter { it.name == "apex_maint_session" && it.httpOnly && it.hostOnly }.forEach { cookie ->
            current.removeAll { it.name == cookie.name && it.domain == cookie.domain && it.path == cookie.path }
            if (cookie.expiresAt > System.currentTimeMillis()) current.add(cookie)
        }
        store.put("cookies", current.joinToString("\n") { it.toString() })
    }
    @Synchronized override fun loadForRequest(url: HttpUrl): List<Cookie> =
        store.get("cookies").orEmpty().lineSequence().mapNotNull { Cookie.parse(url, it) }
            .filter { it.expiresAt > System.currentTimeMillis() && it.matches(url) }.toList()
}
