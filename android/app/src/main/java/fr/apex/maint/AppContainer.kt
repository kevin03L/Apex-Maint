package fr.apex.maint

import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import androidx.room.Room
import androidx.work.*
import fr.apex.maint.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.*
import okhttp3.OkHttpClient
import okhttp3.HttpUrl.Companion.toHttpUrl
import java.io.IOException
import java.util.concurrent.TimeUnit

class MaintApplication : Application() {
    val container by lazy { AppContainer(this) }
    override fun onCreate() { super.onCreate(); container.observeNetwork() }
}

class AppContainer(val context: Context) {
    val gate = Mutex()
    val settings = Settings(context)
    val secure = SecureStore(context)
    val expired = MutableStateFlow(false)
    val revision = MutableStateFlow(0)
    val online = MutableStateFlow(isOnline())
    private var apiInstance: MaintApi? = null
    val api get() = requireNotNull(apiInstance) { "Configurez le serveur" }
    private val database = Room.databaseBuilder(context, MaintDatabase::class.java, "terrain.db").build()
    val repo = Repository(database, secure.cipher) { api }
    var session: Session? = null; private set
    var actor: Actor? = null; private set
    var serverUrl: String? = null; private set

    fun isOnline(): Boolean {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        return cm.getNetworkCapabilities(cm.activeNetwork)?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
    }
    fun observeNetwork() {
        context.getSystemService(ConnectivityManager::class.java).registerDefaultNetworkCallback(
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) { online.value = isOnline(); if (online.value) schedule() }
                override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                    val wasOnline = online.value
                    online.value = isOnline()
                    if (online.value && !wasOnline) schedule()
                }
                override fun onLost(network: Network) { online.value = isOnline() }
            })
        val periodic = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()).build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork("terrain_periodique", ExistingPeriodicWorkPolicy.KEEP, periodic)
    }
    private fun configure(url: String) {
        serverUrl = url
        // Refus de toute redirection : aucun identifiant ou document vers une autre origine.
        val client = OkHttpClient.Builder().cookieJar(PersistentCookieJar(secure))
            .followRedirects(false).followSslRedirects(false).retryOnConnectionFailure(false).callTimeout(60, TimeUnit.SECONDS)
            .addInterceptor { chain ->
                val expected = url.toHttpUrl()
                val request = chain.request()
                if (request.url.scheme != expected.scheme || request.url.host != expected.host || request.url.port != expected.port)
                    throw IOException("Origine du serveur refusée")
                val response = chain.proceed(request)
                if (response.code == 401 && !request.url.encodedPath.endsWith("/auth/login")) expired.value = true
                response
            }.build()
        apiInstance = apiAt(url, client)
    }
    suspend fun initialize() = gate.withLock {
        if (serverUrl == null) settings.url()?.let { configure(it) }
        repo.protectCache()
        if (apiInstance == null || expired.value) return@withLock
        if (session == null) secure.get("session")?.let { session = wire.decodeFromString<Session>(it) }
        if (session != null) {
            try { acceptSession(wire.decodeFromJsonElement(requireNotNull(api.get("api/auth/me").checked()))) }
            catch (e: IOException) { /* La dernière session connue autorise la lecture hors ligne. */ }
            catch (e: ApiFailure) { if (e.status != 401) throw e }
            restoreActor()
        }
    }
    suspend fun saveServer(url: String) = gate.withLock {
        require(session == null) { "Déconnectez-vous avant de changer de serveur" }
        val normalized = settings.save(url)
        secure.clear(); configure(normalized); expired.value = false
    }
    suspend fun login(username: String, password: String) = gate.withLock {
        api.post("api/auth/login", obj("username" to username.j(), "password" to password.j()).requestBody()).checked()
        val fresh = wire.decodeFromJsonElement<Session>(requireNotNull(api.get("api/auth/me").checked()))
        acceptSession(fresh); expired.value = false; restoreActor()
    }
    private suspend fun acceptSession(fresh: Session) {
        val old = session ?: secure.get("session")?.let { wire.decodeFromString<Session>(it) }
        if (old == null || old.user.id != fresh.user.id || old.user.site_id != fresh.user.site_id) {
            repo.purge(Actor(old?.user?.id ?: "local", old?.user?.site_id.orEmpty(), old?.permissions.orEmpty()))
            secure.put("site", null)
            clearFiles()
        }
        session = fresh; secure.put("session", wire.encodeToString(fresh))
        restoreActor()
        revision.value++
    }
    private fun restoreActor() {
        val s = session ?: return
        val site = if (s.user.role == "admin") secure.get("site") ?: s.user.site_id else s.user.site_id
        actor = site?.let { Actor(s.user.id, it, s.permissions, s.user.role == "admin") }
    }
    suspend fun selectSite(id: String) = gate.withLock {
        val s = requireNotNull(session)
        require(s.user.role == "admin" || id == s.user.site_id) { "Changement de site refusé" }
        if (actor?.siteId == id) return@withLock
        require(actor?.let { repo.operations(it).isEmpty() } != false) { "Synchronisez ou résolvez la file avant de changer de site" }
        val sites = api.get("api/sites").checked().objects()
        require(sites.any { it.text("id") == id }) { "Site inaccessible" }
        actor?.let { repo.purge(it) }
        clearFiles(); secure.put("site", id); restoreActor(); revision.value++
    }
    suspend fun password(current: String, new: String) = gate.withLock {
        api.post("api/auth/password", obj("current_password" to current.j(), "new_password" to new.j()).requestBody()).checked()
        acceptSession(wire.decodeFromJsonElement(requireNotNull(api.get("api/auth/me").checked())))
    }
    suspend fun logout() = gate.withLock {
        try { api.post("api/auth/logout", obj().requestBody()).checked() }
        catch (_: IOException) { /* La déconnexion locale reste possible sans réseau. */ }
        catch (_: ApiFailure) { /* La session peut déjà être expirée. */ }
        finally {
            val s = session
            repo.purge(actor ?: Actor(s?.user?.id ?: "local", "", emptySet()))
            WorkManager.getInstance(context).cancelUniqueWork("terrain_sync")
            clearFiles(); secure.clear(); session = null; actor = null; expired.value = false; revision.value++
        }
    }
    private fun clearFiles() {
        // Uniquement les fichiers privés temporaires appartenant à ce client.
        listOf("documents", "photos").forEach { java.io.File(context.cacheDir, it).deleteRecursively() }
    }
    fun schedule() {
        val work = OneTimeWorkRequestBuilder<SyncWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build()
        WorkManager.getInstance(context).enqueueUniqueWork("terrain_sync", ExistingWorkPolicy.APPEND_OR_REPLACE, work)
    }
    suspend fun synchronize(): Boolean = gate.withLock {
        repo.protectCache()
        if (serverUrl == null) settings.url()?.let { configure(it) }
        if (apiInstance == null || expired.value) return@withLock true
        if (session == null) secure.get("session")?.let { session = wire.decodeFromString<Session>(it) }
        if (session == null) return@withLock true
        // Identité et permissions courantes vérifiées avant le premier envoi.
        acceptSession(wire.decodeFromJsonElement(requireNotNull(api.get("api/auth/me").checked())))
        if (session?.must_change_password == true || session?.user?.must_change_password == true) return@withLock true
        val a = actor ?: return@withLock true
        val done = repo.sync(a)
        if (done) {
            repo.refresh(a)
            repo.siteInterventions(a)
            if (a.can("parts.view")) repo.allPages(a, "api/parts?site_id=${encode(a.siteId)}", "parts")
        }
        revision.value++
        done
    }
}

class SyncWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val container = (applicationContext as MaintApplication).container
        return try { if (container.synchronize()) Result.success() else Result.retry() }
        catch (_: IOException) { Result.retry() }
        catch (e: ApiFailure) { if (e.status == 401 || e.status == 403) Result.success() else Result.retry() }
        catch (e: CancellationException) { throw e }
        catch (_: Exception) { Result.retry() }
    }
}
