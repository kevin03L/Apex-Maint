package fr.apex.maint.data

import androidx.room.withTransaction
import kotlinx.serialization.json.*
import java.time.Instant
import java.util.UUID

/** Le conteneur sérialise ses appels avec la session et le Worker. */
class Repository(private val db: MaintDatabase, cipher: PayloadCipher, private val api: () -> MaintApi) {
    private val dao = EncryptedDao(db.dao(), cipher)
    private var protected = false
    suspend fun protectCache() {
        if (protected) return
        val sql = db.openHelper.writableDatabase
        sql.query("PRAGMA secure_delete=ON").use { it.moveToFirst() }
        val migrated = db.withTransaction { dao.protectLegacy() }
        if (migrated) {
            sql.query("PRAGMA wal_checkpoint(TRUNCATE)").use { it.moveToFirst() }
            sql.execSQL("VACUUM")
            sql.query("PRAGMA wal_checkpoint(TRUNCATE)").use { it.moveToFirst() }
        }
        protected = true
    }
    suspend fun collection(actor: Actor, name: String): List<JsonObject> =
        dao.rows(actor.scope, name).map { wire.parseToJsonElement(it.payload).jsonObject }
    suspend fun item(actor: Actor, name: String, id: String): JsonObject? =
        dao.row(actor.scope, name, id)?.let { wire.parseToJsonElement(it.payload).jsonObject }
    suspend fun operations(actor: Actor) = dao.operations(actor.scope)
    suspend fun purge(actor: Actor) {
        require(actor.userId.isNotEmpty())
        db.withTransaction { dao.purgeCache(); dao.purgeQueue() }
    }
    private suspend fun save(actor: Actor, collection: String, item: JsonObject) {
        require(item.text("site_id")?.let { it == actor.siteId } != false) { "Site incohérent" }
        dao.cache(listOf(CacheRow(actor.scope, collection, requireNotNull(item.text("id")),
            item.toString(), item.text("deleted_at"))))
    }
    private suspend fun retireItem(actor: Actor, collection: String, id: String) {
        dao.row(actor.scope, collection, id)?.let { dao.cache(listOf(it.copy(deletedAt = Instant.now().toString()))) }
    }
    suspend fun refresh(actor: Actor) {
        if (!actor.can("interventions.view")) return
        val since = item(actor, "meta", "delta")?.text("generated_at")
        val suffix = since?.let { "?since=${encode(it)}" }.orEmpty()
        val result = requireNotNull(api().get("api/offline/snapshot$suffix").checked())
        val snap = wire.decodeFromJsonElement<Snapshot>(result)
        db.withTransaction {
            val pending = operations(actor)
            val protected = pending.flatMap { listOfNotNull(it.dependsOn, it.tempId) }.toSet()
            val groups = mapOf("equipments" to snap.equipments, "production_lines" to snap.production_lines,
                "counters" to snap.counters, "parts" to snap.parts, "users" to snap.users)
            groups.forEach { (name, items) ->
                if (snap.full) dao.retire(actor.scope, name, snap.generated_at)
                items.filter { it.text("site_id")?.let { site -> site == actor.siteId } != false }
                    .forEach { save(actor, name, it) }
            }
            collection(actor, "interventions").forEach { old ->
                val id = requireNotNull(old.text("id"))
                if (!id.startsWith("temp_") && id !in snap.intervention_ids && id !in protected) {
                    retireItem(actor, "interventions", id)
                    retireItem(actor, "server_interventions", id)
                }
            }
            snap.interventions.filter { it.text("site_id") == actor.siteId }.forEach { itv ->
                val id = requireNotNull(itv.text("id"))
                val server = JsonObject(itv + mapOf(
                    "technicians" to JsonArray(snap.technicians.filter { it.text("intervention_id") == id }),
                    "server_updated_at" to (itv["updated_at"] ?: JsonNull)))
                save(actor, "server_interventions", server)
                if (id !in protected || itv.text("deleted_at") != null) save(actor, "interventions", server)
            }
            save(actor, "meta", obj("id" to "delta".j(), "generated_at" to snap.generated_at.j()))
        }
    }
    suspend fun siteInterventions(actor: Actor) {
        if (!actor.can("interventions.view")) return
        allPages(actor, "api/interventions?site_id=${encode(actor.siteId)}", "interventions")
    }
    suspend fun allPages(actor: Actor, path: String, collection: String) {
        var cursor: String? = null
        val seen = mutableSetOf<String>()
        val received = mutableSetOf<String>()
        do {
            val page = api().get("$path&limit=100" + cursor?.let { "&cursor=${encode(it)}" }.orEmpty()).checked().objectOrEmpty()
            db.withTransaction {
                val protected = operations(actor).flatMap { listOfNotNull(it.dependsOn, it.tempId) }.toSet()
                page.list("items").filter { it.text("site_id") == actor.siteId }
                    .forEach {
                        received.add(requireNotNull(it.text("id")))
                        if (it.text("id") !in protected) save(actor, collection, it)
                        if (collection == "interventions") save(actor, "server_interventions", it)
                    }
            }
            cursor = page.text("next_cursor")
            check(cursor == null || seen.add(cursor)) { "Pagination serveur incohérente" }
        } while (cursor != null)
        db.withTransaction {
            val protected = operations(actor).flatMap { listOfNotNull(it.dependsOn, it.tempId) }.toSet()
            this@Repository.collection(actor, collection).filter { it.text("id") !in received && it.text("id") !in protected }
                .forEach { retireItem(actor, collection, requireNotNull(it.text("id"))) }
        }
    }
    suspend fun detail(actor: Actor, collection: String, id: String): JsonObject {
        if (id.startsWith("temp_")) return requireNotNull(item(actor, collection, id))
        val result = api().get("api/$collection/${encode(id)}").checked().objectOrEmpty()
        require(result.text("site_id") == actor.siteId) { "Entité hors du site sélectionné" }
        if (collection == "interventions") save(actor, "server_interventions", result)
        return if (operations(actor).none { it.dependsOn == id }) {
            save(actor, collection, result); result
        } else item(actor, collection, id) ?: result
    }
    suspend fun create(actor: Actor, input: JsonObject): String {
        require(actor.can("interventions.create")) { "Création non autorisée" }
        val id = "temp_${UUID.randomUUID()}"
        val at = Instant.now().toString()
        val body = JsonObject(input + mapOf("site_id" to actor.siteId.j(), "reported_at" to at.j(),
            "technician_ids" to JsonArray(listOf(actor.userId.j()))))
        require(TerrainRules.inputProblem(body) == null) { TerrainRules.inputProblem(body).orEmpty() }
        db.withTransaction {
            save(actor, "interventions", JsonObject(body + mapOf("id" to id.j(),
                "code" to "Référence provisoire".j(), "status" to (if (input.text("type") == "a_posteriori") "completed" else "open").j(), "updated_at" to at.j(),
                "server_updated_at" to JsonNull, "technicians" to JsonArray(listOf(obj(
                    "user_id" to actor.userId.j(), "duration_minutes" to (input["duration_minutes"] ?: JsonNull)))))))
            dao.operation(Operation(scope = actor.scope, userId = actor.userId, siteId = actor.siteId,
                kind = "create_intervention", url = "/api/interventions", method = "POST", body = body.toString(),
                tempId = id, occurredAt = at, position = dao.nextPosition(actor.scope)))
        }
        return id
    }
    suspend fun mutate(actor: Actor, id: String, kind: String, body: JsonObject) {
        require(actor.can("interventions.edit")) { "Modification non autorisée" }
        require(TerrainRules.inputProblem(body) == null) { TerrainRules.inputProblem(body).orEmpty() }
        db.withTransaction {
            val old = requireNotNull(item(actor, "interventions", id)) { "Rechargez l’intervention" }
            if (kind == "complete_intervention") require(TerrainRules.completionProblem(old, body.text("solution_report").orEmpty()) == null) {
                TerrainRules.completionProblem(old, body.text("solution_report").orEmpty()).orEmpty()
            }
            val at = Instant.now().toString()
            val version = if (id.startsWith("temp_")) null else
                if (old.containsKey("server_updated_at")) old.text("server_updated_at") else old.text("updated_at")
            val (suffix, method) = when (kind) {
                "take_intervention" -> "/take" to "POST"
                "record_time" -> "/technicians/${encode(actor.userId)}/time" to "PUT"
                "complete_intervention" -> "/complete" to "POST"
                "patch_intervention" -> "" to "PATCH"
                else -> error("Action terrain inconnue")
            }
            val operation = Operation(scope = actor.scope, userId = actor.userId, siteId = actor.siteId,
                kind = kind, url = "/api/interventions/${encode(id)}$suffix", method = method,
                body = QueueProtocol.bodyForActor(actor, kind, body).toString(),
                occurredAt = at, dependsOn = id, version = version,
                position = dao.nextPosition(actor.scope))
            if (!id.startsWith("temp_") && item(actor, "server_interventions", id) == null)
                save(actor, "server_interventions", old)
            save(actor, "interventions", LocalProjection.apply(old, operation))
            dao.operation(operation)
        }
    }
    suspend fun sync(actor: Actor): Boolean {
        return QueueRunner.execute(actor, { operations(actor) }, send = { op ->
            val body = wire.parseToJsonElement(op.body).jsonObject.requestBody()
            val path = op.url.removePrefix("/")
            val response = when (op.method) {
                "PATCH" -> api().patch(path, body, op.headers())
                "PUT" -> api().put(path, body, op.headers())
                else -> api().post(path, body, op.headers())
            }
            response.checked().objectOrEmpty()
        }, acknowledge = { op, reply ->
                db.withTransaction {
                    val all = operations(actor)
                    QueueProtocol.acknowledge(op, reply, all).filter { it.id != op.id }.forEach { dao.operation(it) }
                    val realId = reply.text("id")
                    if (op.tempId != null && realId != null) {
                        val local = item(actor, "interventions", op.tempId)
                        retireItem(actor, "interventions", op.tempId)
                        val localFields = if (all.any { it.id != op.id && it.dependsOn == op.tempId })
                            local.orEmpty().filterKeys { it !in setOf("id", "code", "site_id", "server_updated_at") }
                            else emptyMap()
                        save(actor, "interventions", JsonObject(reply + localFields))
                        save(actor, "aliases", obj("id" to op.tempId.j(), "real_id" to realId.j()))
                    }
                    val serverId = if (op.kind == "record_time") reply.text("intervention_id") else realId
                    if (serverId != null && QueueProtocol.acknowledgedId(op, reply) != null) {
                        val base = if (op.kind == "record_time") {
                            item(actor, "server_interventions", serverId)?.let { LocalProjection.apply(it, op) }
                        } else reply
                        if (base != null) {
                            val server = JsonObject(base + mapOf("updated_at" to (reply["updated_at"] ?: JsonNull),
                                "server_updated_at" to (reply["updated_at"] ?: JsonNull)))
                            save(actor, "server_interventions", server)
                            val remaining = operations(actor).filter { it.id != op.id && it.dependsOn == serverId }
                            // Une ancienne réponse sans condition ne réinitialise pas la version du cache.
                            if (QueueProtocol.acknowledgedId(op, reply) != null)
                                save(actor, "interventions", LocalProjection.replay(server, remaining))
                        }
                    }
                    QueueProtocol.acknowledgedId(op, reply)?.let { entityId ->
                        item(actor, "interventions", entityId)?.let { old -> save(actor, "interventions",
                            JsonObject(old + ("server_updated_at" to reply["updated_at"]!!))) }
                    }
                    dao.operation(op.copy(state = "sent", deletedAt = Instant.now().toString()))
                }
        }, conflict = { op, failure ->
            dao.operation(op.copy(state = "conflict", error = failure.message, errorStatus = failure.status))
        })
    }
    suspend fun resolve(actor: Actor, id: String, retry: Boolean, reviewedVersion: String? = null) {
        val op = operations(actor).first { it.id == id && it.state == "conflict" }
        if (!retry) {
            db.withTransaction {
                val target = op.tempId ?: op.dependsOn
                operations(actor).filter { it.id == id || (op.tempId != null && it.dependsOn == op.tempId) }
                    .forEach { dao.operation(it.copy(state = "abandoned", deletedAt = Instant.now().toString())) }
                if (op.tempId != null) retireItem(actor, "interventions", op.tempId)
                else if (target != null) {
                    item(actor, "server_interventions", target)?.let { server ->
                        save(actor, "interventions", LocalProjection.replay(server,
                            operations(actor).filter { it.dependsOn == target }))
                    }
                }
            }
            return
        }
        val fresh = op.dependsOn?.takeUnless { it.startsWith("temp_") }?.let {
            api().get("api/interventions/${encode(it)}").checked().objectOrEmpty()
        }
        require(fresh == null || fresh.text("updated_at") == reviewedVersion) {
            "L’intervention a encore changé. Rechargez et comparez à nouveau."
        }
        db.withTransaction {
            if (fresh != null) save(actor, "server_interventions", fresh)
            // Un refus 4xx est mémorisé côté serveur : une nouvelle tentative doit avoir une nouvelle clé.
            dao.operation(op.copy(state = "replaced", deletedAt = Instant.now().toString()))
            dao.operation(op.copy(id = UUID.randomUUID().toString(), state = "pending", error = null,
                errorStatus = null, version = fresh?.text("updated_at")))
            if (fresh != null) save(actor, "interventions", LocalProjection.replay(fresh,
                operations(actor).filter { it.dependsOn == op.dependsOn }))
        }
    }
    suspend fun get(actor: Actor, path: String): JsonElement? {
        require(actor.siteId.isNotEmpty()); return api().get(path).checked()
    }
    suspend fun consume(actor: Actor, interventionId: String, partId: String, locationId: String, quantity: Int) {
        require(actor.can("stock.movements")) { "Sortie de pièces non autorisée" }
        require(quantity > 0) { "La quantité doit être positive" }
        detail(actor, "interventions", interventionId)
        api().post("api/interventions/${encode(interventionId)}/parts", obj("part_id" to partId.j(),
            "location_id" to locationId.j(), "quantity" to quantity.j()).requestBody()).checked()
    }
}
fun encode(value: String): String = java.net.URLEncoder.encode(value, "UTF-8").replace("+", "%20")
