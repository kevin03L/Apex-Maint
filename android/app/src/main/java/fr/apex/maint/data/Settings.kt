package fr.apex.maint.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import fr.apex.maint.BuildConfig
import kotlinx.coroutines.flow.first
import okhttp3.HttpUrl.Companion.toHttpUrl

private val Context.settings by preferencesDataStore("serveur")
class Settings(private val context: Context) {
    private val server = stringPreferencesKey("url")
    suspend fun url() = context.settings.data.first()[server]
    suspend fun save(value: String): String {
        val url = runCatching { value.trim().toHttpUrl() }.getOrElse {
            throw IllegalArgumentException("URL invalide. Indiquez l’adresse HTTPS complète du serveur.")
        }
        require(url.username.isEmpty() && url.password.isEmpty() && url.query == null &&
            url.fragment == null && url.encodedPath == "/") { "Indiquez l’URL racine, sans identifiants ni chemin" }
        require(url.isHttps || (BuildConfig.DEBUG && url.host in setOf("localhost", "10.0.2.2"))) {
            "Une URL HTTPS est obligatoire"
        }
        return url.toString().also { normalized -> context.settings.edit { it[server] = normalized } }
    }
}
