package fr.apex.maint.data

import kotlinx.serialization.json.JsonObject
import java.time.Instant
import java.util.UUID

/** La projection et l'invalidation persistante des brouillons partagent la transaction de résolution. */
class ConflictResolver(private val dao: MaintDao, private val api: () -> MaintApi,
    private val transaction: suspend (suspend () -> Unit) -> Unit) {
    private suspend fun save(actor: Actor, collection: String, item: JsonObject) {
        require(item.text("site_id")?.let { it == actor.siteId } != false) { "Site incohérent" }
        dao.cache(listOf(CacheRow(actor.scope, collection, requireNotNull(item.text("id")), item.toString())))
    }
    suspend fun resolve(actor: Actor, id: String, retry: Boolean, reviewedVersion: String?) {
        val op = dao.operations(actor.scope).first { it.id == id && it.state == "conflict" }
        val target = op.tempId ?: op.dependsOn
        val fresh = op.dependsOn?.takeUnless { it.startsWith("temp_") }?.let {
            api().get("api/interventions/${encode(it)}").checked().objectOrEmpty().also { remote ->
                require(remote.text("site_id") == actor.siteId && remote.text("id") == it) { "Entité hors du site sélectionné" }
            }
        }
        require(!retry || fresh == null || fresh.text("updated_at") == reviewedVersion) {
            "L’intervention a encore changé. Rechargez et comparez à nouveau."
        }
        transaction {
            if (fresh != null) save(actor, "server_interventions", fresh)
            if (target != null) save(actor, "draft_revisions", obj("id" to target.j(), "revision" to UUID.randomUUID().toString().j()))
            val now = Instant.now().toString()
            if (retry) {
                dao.operation(op.copy(state = "replaced", deletedAt = now))
                dao.operation(op.copy(id = UUID.randomUUID().toString(), state = "pending", error = null,
                    errorStatus = null, version = fresh?.text("updated_at")))
            } else {
                dao.operations(actor.scope).filter { it.id == id || (op.tempId != null && it.dependsOn == op.tempId) }
                    .forEach { dao.operation(it.copy(state = "abandoned", deletedAt = now)) }
            }
            if (!retry && op.tempId != null) {
                dao.row(actor.scope, "interventions", requireNotNull(op.tempId))?.let { dao.cache(listOf(it.copy(deletedAt = now))) }
            } else if (target != null && fresh != null) {
                save(actor, "interventions", LocalProjection.replay(fresh,
                    dao.operations(actor.scope).filter { it.dependsOn == target }))
            }
        }
    }
}
