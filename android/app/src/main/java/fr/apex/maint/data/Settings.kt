package fr.apex.maint.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import fr.apex.maint.BuildConfig
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.map
import okhttp3.HttpUrl.Companion.toHttpUrl
import java.io.IOException

private val Context.settings by preferencesDataStore("serveur")
enum class ThemeChoice(val stored: String, val title: String) {
    SYSTEM("system", "Système"), LIGHT("light", "Clair"), DARK("dark", "Sombre");

    fun isDark(systemDark: Boolean) = when (this) {
        SYSTEM -> systemDark
        LIGHT -> false
        DARK -> true
    }
    companion object {
        fun fromStored(value: String?) = entries.find { it.stored == value } ?: DARK
    }
}

class Settings internal constructor(private val store: DataStore<Preferences>) {
    constructor(context: Context) : this(context.settings)
    private val server = stringPreferencesKey("url")
    private val themeKey = stringPreferencesKey("theme")
    val theme = store.data.map { ThemeChoice.fromStored(it[themeKey]) }.catch {
        // Une préférence inaccessible ne doit pas masquer les erreurs de connexion/stockage de l’écran.
        if (it is IOException) emit(ThemeChoice.DARK) else throw it
    }
    suspend fun saveTheme(value: ThemeChoice) { store.edit { it[themeKey] = value.stored } }
    suspend fun url() = store.data.first()[server]
    suspend fun save(value: String): String {
        val url = runCatching { value.trim().toHttpUrl() }.getOrElse {
            throw IllegalArgumentException("URL invalide. Indiquez l’adresse HTTPS complète du serveur.")
        }
        require(url.username.isEmpty() && url.password.isEmpty() && url.query == null &&
            url.fragment == null && url.encodedPath == "/") { "Indiquez l’URL racine, sans identifiants ni chemin" }
        require(url.isHttps || (BuildConfig.DEBUG && url.host in setOf("localhost", "10.0.2.2"))) {
            "Une URL HTTPS est obligatoire"
        }
        return url.toString().also { normalized -> store.edit { it[server] = normalized } }
    }
}
