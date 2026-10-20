package fr.apex.maint.ui

import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.viewModelScope
import fr.apex.maint.AppContainer
import fr.apex.maint.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.*
import java.io.IOException
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

data class UiState(
    val ready: Boolean = false, val server: String? = null, val session: Session? = null,
    val actor: Actor? = null, val sites: List<JsonObject> = emptyList(),
    val interventions: List<JsonObject> = emptyList(), val equipments: List<JsonObject> = emptyList(),
    val parts: List<JsonObject> = emptyList(), val operations: List<Operation> = emptyList(),
    val selected: JsonObject? = null, val children: List<JsonObject> = emptyList(),
    val consumed: List<JsonObject> = emptyList(), val documents: List<JsonObject> = emptyList(),
    val resourceKey: String? = null, val busy: Boolean = false, val message: String? = null,
    val expired: Boolean = false, val online: Boolean = false, val formEquipment: String? = null,
    val lastSync: String? = null,
    val refreshing: Boolean = false,
    val aliases: Map<String, String> = emptyMap(),
)

class MaintViewModel(val container: AppContainer, private val saved: SavedStateHandle) : ViewModel() {
    val mine = saved.getStateFlow("mine", true)
    val status = saved.getStateFlow("status", "")
    fun filterMine(value: Boolean) { saved["mine"] = value }
    fun filterStatus(value: String) { saved["status"] = value }
    private val mutable = MutableStateFlow(UiState())
    val state = mutable.asStateFlow()
    private val selection = java.util.concurrent.atomic.AtomicLong()
    private val running = java.util.concurrent.atomic.AtomicInteger()
    private var selectionJob: Job? = null
    private val documents = Documents(container.context) { container.api }
    init {
        viewModelScope.launch { container.expired.collect { expired ->
            mutable.update { it.copy(expired = expired) }
        } }
        viewModelScope.launch { container.online.collect { online -> mutable.update { it.copy(online = online) } } }
        viewModelScope.launch { container.revision.drop(1).collect { if (mutable.value.ready) reload() } }
        run {
            container.initialize(); reload()
            if (container.session != null) container.schedule()
        }
    }
    private fun run(networkMessage: String? = null, work: suspend () -> Unit): Job = viewModelScope.launch {
            running.incrementAndGet()
            mutable.update { it.copy(busy = true, message = null) }
            try { withContext(Dispatchers.IO) { work() } }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                if (e is ApiFailure && e.code == "password_change_required")
                    mutable.update { it.copy(session = it.session?.copy(must_change_password = true)) }
                val message = if (networkMessage != null && e is IOException && e !is LocalStorageFailure) networkMessage else when (e) {
                    is javax.net.ssl.SSLException -> "Certificat refusé. Vérifiez l’autorité installée et le nom du serveur."
                    is java.net.UnknownHostException -> "Nom du serveur introuvable. Vérifiez l’URL et le réseau."
                    is java.net.SocketTimeoutException -> "Le serveur ne répond pas dans le délai prévu."
                    is LocalStorageFailure -> e.message.orEmpty()
                    is IOException -> "Connexion impossible. Vérifiez le réseau puis réessayez."
                    is kotlinx.serialization.SerializationException -> "La réponse du serveur est illisible."
                    is android.content.ActivityNotFoundException -> "Aucune application ne peut ouvrir ce document."
                    is java.security.GeneralSecurityException -> "Le stockage chiffré est inaccessible. Contactez votre administrateur avant de réinitialiser l’application."
                    is ApiFailure -> e.message
                    is IllegalArgumentException -> e.message ?: "Valeur invalide"
                    is IllegalStateException -> e.message?.takeUnless { it == "Required value was null." } ?: "Action impossible"
                    else -> "Action impossible. Réessayez ou contactez votre administrateur."
                }
                mutable.update { it.copy(message = message, server = it.server ?: container.serverUrl) }
            }
            finally { val busy = running.decrementAndGet() > 0; mutable.update { it.copy(busy = busy, ready = true) } }
    }
    suspend fun reload() = withContext(Dispatchers.IO) {
        container.gate.withLock {
            val actor = container.actor
            val session = container.session
            val oldScope = mutable.value.actor?.scope
            val interventions = actor?.let { container.repo.collection(it, "interventions") }.orEmpty()
            val equipments = actor?.let { container.repo.collection(it, "equipments") }.orEmpty()
            val parts = actor?.let { container.repo.collection(it, "parts") }.orEmpty()
            val operations = actor?.let { container.repo.operations(it) }.orEmpty()
            val lastSync = actor?.let { container.repo.item(it, "meta", "delta")?.text("generated_at") }
            val aliases = actor?.let { a -> container.repo.collection(a, "aliases").associate {
                requireNotNull(it.text("id")) to requireNotNull(it.text("real_id")) } }.orEmpty()
            val selected = if (actor?.scope == oldScope) mutable.value.resourceKey?.let { key ->
                val collection = key.substringBefore(':'); val id = key.substringAfter(':')
                actor?.let { container.repo.item(it, collection, id) }
            } else null
            mutable.update { old -> old.copy(ready = true, server = container.serverUrl, session = session, actor = actor,
                interventions = interventions, equipments = equipments, parts = parts,
                operations = operations, lastSync = lastSync, aliases = aliases,
                selected = selected,
                documents = old.documents.takeIf { actor?.scope == oldScope }.orEmpty(),
                consumed = old.consumed.takeIf { actor?.scope == oldScope }.orEmpty(),
                children = old.children.takeIf { actor?.scope == oldScope }.orEmpty()) }
            withContext(Dispatchers.Main) {
                if (saved.get<String>("scope") != actor?.scope) {
                    saved.remove<String>("formEquipment"); saved.remove<String>("resourceKey")
                }
                saved["scope"] = actor?.scope
                mutable.update { it.copy(formEquipment = saved["formEquipment"]) }
            }
        }
    }
    fun message(value: String?) { mutable.update { it.copy(message = value) } }
    fun server(value: String) = run { container.saveServer(value); reload(); message("Serveur enregistré") }
    fun login(user: String, password: String) = run {
        container.login(user, password); reload(); container.schedule(); message("Connexion établie")
    }
    fun password(current: String, new: String) = run { container.password(current, new); reload(); container.schedule(); message("Mot de passe modifié") }
    fun logout() = run { container.logout(); mutable.value = UiState(online = container.online.value); reload() }
    fun selectSite(id: String) = run { container.selectSite(id); reload(); container.schedule() }
    fun sites() = run {
        val list = container.gate.withLock { container.api.get("api/sites").checked().objects() }
        mutable.update { it.copy(sites = list) }
    }
    private fun refresh(work: suspend () -> Unit) = run {
        mutable.update { it.copy(refreshing = true) }
        try { work() } finally { mutable.update { it.copy(refreshing = false) } }
    }
    fun sync() = refresh {
        val done = container.synchronize(); reload()
        mutable.update { it.copy(message = if (done) "Synchronisation terminée : données actualisées" else "Envoi interrompu : une nouvelle tentative est prévue") }
        if (!done) container.schedule()
    }
    fun siteList() = refresh {
        container.gate.withLock { container.repo.siteInterventions(requireNotNull(container.actor)) }; reload()
        mutable.update { it.copy(message = "Interventions du site actualisées") }
    }
    fun catalogue() = refresh {
        container.gate.withLock {
            val a = requireNotNull(container.actor)
            if (a.can("parts.view")) container.repo.allPages(a, "api/parts?site_id=${encode(a.siteId)}", "parts")
        }; reload(); message("Catalogue actualisé")
    }
    fun select(collection: String, id: String) {
        val request = selection.incrementAndGet()
        selectionJob?.cancel()
        saved["resourceKey"] = "$collection:$id"
        selectionJob = run {
            container.gate.withLock {
                if (request != selection.get()) return@withLock
                val a = requireNotNull(container.actor)
                val key = "$collection:$id"
                mutable.update { old ->
                    if (old.resourceKey == key) old else old.copy(resourceKey = key, selected = null,
                        documents = emptyList(), children = emptyList(), consumed = emptyList())
                }
                val cached = container.repo.item(a, collection, id)
                mutable.update { it.copy(selected = cached) }
                val item = if (container.online.value && !id.startsWith("temp_")) {
                    try { container.repo.detail(a, collection, id) }
                    catch (e: IOException) { cached ?: throw e }
                } else cached
                currentCoroutineContext().ensureActive()
                mutable.update { it.copy(selected = item) }
                if (container.online.value && !id.startsWith("temp_")) {
                    if (a.can("documents.view") && collection in setOf("equipments", "interventions")) optionalResource {
                        val type = if (collection == "equipments") "equipment" else "intervention"
                        val docs = container.repo.get(a, "api/documents?entity_type=$type&entity_id=${encode(id)}").objects()
                        mutable.update { it.copy(documents = docs) }
                    }
                    if (collection == "interventions") {
                        optionalResource {
                    val children = container.repo.get(a, "api/interventions/${encode(id)}/sub-interventions").objects()
                        mutable.update { it.copy(children = children) }
                        }
                        if (a.can("stock.view")) optionalResource {
                            val used = container.repo.get(a, "api/interventions/${encode(id)}/parts").objects()
                            mutable.update { it.copy(consumed = used) }
                        }
                    }
                }
            }
    }
    }
    private suspend fun optionalResource(load: suspend () -> Unit) {
        try { load() }
        catch (e: IOException) { mutable.update { it.copy(message = "Chargement partiel : vérifiez la connexion puis rechargez la fiche") } }
        catch (e: ApiFailure) {
            if (e.status == 401) throw e
            mutable.update { it.copy(message = e.message) }
        }
        catch (_: kotlinx.serialization.SerializationException) { message("Chargement partiel : une réponse annexe est illisible") }
    }
    fun create(body: JsonObject, done: (String) -> Unit) = run {
        val id = container.gate.withLock { container.repo.create(requireNotNull(container.actor), body) }
        reload(); container.schedule(); withContext(Dispatchers.Main) { done(id) }
        mutable.update { it.copy(message = "Intervention enregistrée sur l’appareil") }
    }
    fun mutate(id: String, kind: String, body: JsonObject) = run {
        container.gate.withLock { container.repo.mutate(requireNotNull(container.actor), id, kind, body) }
        reload(); withContext(Dispatchers.Main) { if (mutable.value.resourceKey == "interventions:$id") select("interventions", id) }; container.schedule()
        mutable.update { it.copy(message = "Action enregistrée : envoi automatique dès que possible") }
    }
    fun review(op: Operation, done: (JsonObject?) -> Unit) = run {
        val fresh = container.gate.withLock {
            val a = requireNotNull(container.actor)
            require(op.scope == a.scope) { "Cette opération appartient à un autre compte ou site" }
            op.dependsOn?.takeUnless { it.startsWith("temp_") }?.let {
                container.repo.get(a, "api/interventions/${encode(it)}").objectOrEmpty()
            }
        }
        withContext(Dispatchers.Main) { done(fresh) }
    }
    fun resolve(id: String, retry: Boolean, reviewedVersion: String? = null) = run {
        container.gate.withLock { container.repo.resolve(requireNotNull(container.actor), id, retry, reviewedVersion) }
        reload(); container.schedule()
    }
    fun consume(interventionId: String, partId: String, locationId: String, quantity: Int) = run {
        container.gate.withLock {
            container.repo.consume(requireNotNull(container.actor), interventionId, partId, locationId, quantity)
        }
        withContext(Dispatchers.Main) { if (mutable.value.resourceKey == "interventions:$interventionId") select("interventions", interventionId) }
        message("Sortie de pièce enregistrée sur le serveur")
    }
    fun part(id: String, done: (JsonObject) -> Unit) = run {
        val result = container.gate.withLock { container.repo.detail(requireNotNull(container.actor), "parts", id) }
        withContext(Dispatchers.Main) { done(result) }
    }
    fun upload(id: String, uri: Uri, photo: Boolean) = run {
        try { container.gate.withLock {
            val a = requireNotNull(container.actor)
            container.repo.detail(a, "interventions", id)
            documents.upload(a, id, uri, photo)
        } } finally { if (photo) java.io.File(requireNotNull(uri.path)).delete() }
        withContext(Dispatchers.Main) { if (mutable.value.resourceKey == "interventions:$id") select("interventions", id) }
        message("Document ajouté sur le serveur")
    }
    fun photoCaptured(id: String, uri: Uri) {
        // Déplacer avant onResume, même si l’envoi attend le verrou de session.
        try { upload(id, documents.claimPhoto(uri), true) }
        catch (_: Exception) { documents.deletePhoto(uri); message("Capture indisponible") }
    }
    fun openDocument(document: JsonObject) = run {
        val version = document["current_version"] as? JsonObject ?: error("Version du document absente")
        val file = container.gate.withLock {
            val actor = requireNotNull(container.actor)
            require(document.text("site_id") == actor.siteId) { "Document hors du site sélectionné" }
            documents.download(actor, requireNotNull(document.text("id")), version.text("extension") ?: "bin")
        }
        withContext(Dispatchers.Main) { documents.open(file, version.text("mime_type") ?: "application/octet-stream") }
    }
    fun equipmentForForm(id: String?) { saved["formEquipment"] = id; mutable.update { it.copy(formEquipment = id) } }
    fun scan(value: String, done: (String, String) -> Unit) = run {
        container.gate.withLock {
            val a = requireNotNull(container.actor)
            val input = value.trim()
            require(input.isNotEmpty() && input.length <= 2048) { "Code vide ou trop long" }
            // Le QR web contient le code seul. Une URL de cette même instance est aussi acceptée.
            val uri = Uri.parse(input)
            val server = requireNotNull(container.serverUrl).toHttpUrl()
            if (input.contains("://")) {
                val link = input.toHttpUrlOrNull()
                require(link != null && link.scheme == server.scheme && link.host == server.host && link.port == server.port &&
                    link.username.isEmpty() && link.password.isEmpty()) {
                    "Ce QR appartient à un autre serveur"
                }
                val path = uri.fragment?.removePrefix("/") ?: uri.path.orEmpty().removePrefix("/")
                val pieces = path.split('/')
                require(pieces.size == 2 && pieces[0] in setOf("equipments", "interventions")) { "Format du QR inconnu" }
                val target = container.repo.item(a, pieces[0], pieces[1]) ?: container.repo.detail(a, pieces[0], pieces[1])
                withContext(Dispatchers.Main) { done(pieces[0], requireNotNull(target.text("id"))) }
            } else {
                var matches = container.repo.collection(a, "equipments").filter { it.text("code").equals(input, true) }
                if (matches.isEmpty() && container.online.value) {
                    val response = container.repo.get(a, "api/equipments/by-code/${encode(input)}")
                    matches = (if (response is JsonObject) listOf(response) else response.objects())
                        .filter { it.text("site_id") == a.siteId }
                }
                val equipment = matches.singleOrNull() ?: error("Équipement inconnu sur ce site")
                val id = requireNotNull(equipment.text("id"))
                mutable.update { it.copy(formEquipment = id) }
                withContext(Dispatchers.Main) { done("equipments", id) }
            }
        }
    }
}
