package fr.apex.maint.ui

import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.apex.maint.AppContainer
import fr.apex.maint.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.*
import java.io.IOException

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
    val aliases: Map<String, String> = emptyMap(),
)

class MaintViewModel(val container: AppContainer) : ViewModel() {
    private val mutable = MutableStateFlow(UiState())
    val state = mutable.asStateFlow()
    private val selection = java.util.concurrent.atomic.AtomicLong()
    private val documents = Documents(container.context) { container.api }
    init {
        viewModelScope.launch { container.expired.collect { expired ->
            mutable.update { it.copy(expired = expired) }
        } }
        viewModelScope.launch { container.online.collect { online -> mutable.update { it.copy(online = online) } } }
        viewModelScope.launch { container.revision.collect { reload() } }
        run { container.initialize(); reload(); if (container.session != null) container.schedule() }
    }
    private fun run(work: suspend () -> Unit) {
        viewModelScope.launch {
            mutable.update { it.copy(busy = true, message = null) }
            try { withContext(Dispatchers.IO) { work() } }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                val message = when (e) {
                    is javax.net.ssl.SSLException -> "Certificat refusé. Vérifiez l’autorité installée et le nom du serveur."
                    is java.net.UnknownHostException -> "Nom du serveur introuvable. Vérifiez l’URL et le réseau."
                    is java.net.SocketTimeoutException -> "Le serveur ne répond pas dans le délai prévu."
                    is IOException -> "Connexion impossible. Les opérations restent enregistrées sur cet appareil."
                    is kotlinx.serialization.SerializationException -> "La réponse du serveur est illisible."
                    is android.content.ActivityNotFoundException -> "Aucune application ne peut ouvrir ce document."
                    else -> e.message?.takeUnless { it == "Required value was null." } ?: "Action impossible"
                }
                mutable.update { it.copy(message = message) }
            }
            finally { mutable.update { it.copy(busy = false, ready = true) } }
        }
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
        }
    }
    fun message(value: String?) { mutable.update { it.copy(message = value) } }
    fun server(value: String) = run { container.saveServer(value); reload() }
    fun login(user: String, password: String) = run {
        container.login(user, password); reload(); container.schedule()
    }
    fun password(current: String, new: String) = run { container.password(current, new); reload(); container.schedule() }
    fun logout() = run { container.logout(); mutable.value = UiState(online = container.online.value); reload() }
    fun selectSite(id: String) = run { container.selectSite(id); reload(); container.schedule() }
    fun sites() = run {
        val list = container.gate.withLock { container.api.get("api/sites").checked().objects() }
        mutable.update { it.copy(sites = list) }
    }
    fun sync() = run { container.synchronize(); reload() }
    fun siteList() = run {
        container.gate.withLock { container.repo.siteInterventions(requireNotNull(container.actor)) }; reload()
    }
    fun catalogue() = run {
        container.gate.withLock {
            val a = requireNotNull(container.actor)
            if (a.can("parts.view")) container.repo.allPages(a, "api/parts?site_id=${encode(a.siteId)}", "parts")
        }; reload()
    }
    fun select(collection: String, id: String) {
        val request = selection.incrementAndGet()
        run {
            container.gate.withLock {
                if (request != selection.get()) return@withLock
                val a = requireNotNull(container.actor)
                val key = "$collection:$id"
                mutable.update { it.copy(resourceKey = key, selected = null, documents = emptyList(),
                    children = emptyList(), consumed = emptyList()) }
                val cached = container.repo.item(a, collection, id)
                mutable.update { it.copy(selected = cached) }
                val item = if (container.online.value && !id.startsWith("temp_")) {
                    try { container.repo.detail(a, collection, id) }
                    catch (e: IOException) { cached ?: throw e }
                } else cached
                mutable.update { it.copy(selected = item) }
                if (container.online.value && !id.startsWith("temp_")) {
                    if (a.can("documents.view") && collection in setOf("equipments", "interventions")) {
                        val type = if (collection == "equipments") "equipment" else "intervention"
                        val docs = container.repo.get(a, "api/documents?entity_type=$type&entity_id=${encode(id)}").objects()
                        mutable.update { it.copy(documents = docs) }
                    }
                    if (collection == "interventions") {
                        val children = container.repo.get(a, "api/interventions/${encode(id)}/sub-interventions").objects()
                        mutable.update { it.copy(children = children) }
                        if (a.can("stock.view")) {
                            val used = container.repo.get(a, "api/interventions/${encode(id)}/parts").objects()
                            mutable.update { it.copy(consumed = used) }
                        }
                    }
                }
            }
    }
    }
    fun create(body: JsonObject, done: (String) -> Unit) = run {
        val id = container.gate.withLock { container.repo.create(requireNotNull(container.actor), body) }
        reload(); container.schedule(); withContext(Dispatchers.Main) { done(id) }
    }
    fun mutate(id: String, kind: String, body: JsonObject) = run {
        container.gate.withLock { container.repo.mutate(requireNotNull(container.actor), id, kind, body) }
        reload(); select("interventions", id); container.schedule()
    }
    fun review(op: Operation, done: (JsonObject?) -> Unit) = run {
        val fresh = container.gate.withLock {
            val a = requireNotNull(container.actor)
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
        select("interventions", interventionId)
    }
    fun part(id: String, done: (JsonObject) -> Unit) = run {
        val result = container.gate.withLock { container.repo.detail(requireNotNull(container.actor), "parts", id) }
        withContext(Dispatchers.Main) { done(result) }
    }
    fun upload(id: String, uri: Uri, photo: Boolean) = run {
        container.gate.withLock {
            val a = requireNotNull(container.actor)
            container.repo.detail(a, "interventions", id)
            documents.upload(a, id, uri, photo)
        }
        select("interventions", id)
    }
    fun openDocument(document: JsonObject) = run {
        val version = document["current_version"]?.jsonObject ?: error("Version du document absente")
        val file = container.gate.withLock { documents.download(requireNotNull(container.actor),
            requireNotNull(document.text("id")), version.text("extension") ?: "bin") }
        withContext(Dispatchers.Main) { documents.open(file, version.text("mime_type") ?: "application/octet-stream") }
    }
    fun equipmentForForm(id: String?) { mutable.update { it.copy(formEquipment = id) } }
    fun scan(value: String, done: (String, String) -> Unit) = run {
        container.gate.withLock {
            val a = requireNotNull(container.actor)
            val input = value.trim()
            // Le QR web contient le code seul. Une URL de cette même instance est aussi acceptée.
            val uri = Uri.parse(input)
            val server = Uri.parse(container.serverUrl)
            if (uri.scheme != null) {
                require(uri.scheme == server.scheme && uri.host == server.host && uri.port == server.port) {
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
