package fr.apex.maint.data

import androidx.room.Entity
import androidx.room.PrimaryKey
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*
import java.time.Instant
import java.util.UUID

@Serializable
@Entity(tableName = "operations")
data class Operation(
    @PrimaryKey val id: String = UUID.randomUUID().toString(),
    val scope: String, val userId: String, val siteId: String,
    val kind: String, val url: String, val method: String, val body: String,
    val occurredAt: String = Instant.now().toString(),
    val dependsOn: String? = null, val tempId: String? = null,
    val version: String? = null, val state: String = "pending", val error: String? = null,
    val errorStatus: Int? = null, val deletedAt: String? = null,
    val position: Long = 0,
)
{
    fun headers(): Map<String, String> = buildMap {
        put("Idempotency-Key", id); put("X-Occurred-At", occurredAt)
        version?.let { put("If-Unmodified-Since", it) }
    }
}

/** Envoi séquentiel ; les acquittements sont persistés avant l’opération suivante. */
object QueueRunner {
    suspend fun execute(actor: Actor, read: suspend () -> List<Operation>,
        send: suspend (Operation) -> JsonObject,
        acknowledge: suspend (Operation, JsonObject) -> Unit,
        conflict: suspend (Operation, ApiFailure) -> Unit): Boolean {
        require(QueueProtocol.owns(actor, read())) { "La file appartient à une autre identité" }
        val blocked = QueueProtocol.blocked(read()).toMutableSet()
        val ids = read().filter { it.state == "pending" }.map { it.id }
        for (id in ids) {
            val op = read().find { it.id == id } ?: continue
            if (op.dependsOn in blocked || op.dependsOn?.startsWith("temp_") == true) continue
            try {
                val reply = send(op)
                if ((op.tempId != null || op.version != null) && QueueProtocol.acknowledgedId(op, reply) == null)
                    throw java.io.IOException("Réponse de synchronisation incomplète ; opération conservée")
                acknowledge(op, reply)
            }
            catch (failure: ApiFailure) {
                if (failure.code == "password_change_required") throw failure
                when (disposition(failure.status)) {
                    HttpDisposition.SESSION -> throw failure
                    HttpDisposition.RETRY -> return false
                    HttpDisposition.CONFLICT -> {
                        conflict(op, failure)
                        listOfNotNull(op.dependsOn, op.tempId).forEach { blocked.add(it) }
                    }
                }
            }
        }
        return true
    }
}

/** Transformations pures partagées par Room et les tests JVM. */
object QueueProtocol {
    fun bodyForActor(actor: Actor, kind: String, body: JsonObject): JsonObject =
        if (kind == "take_intervention") obj("technician_ids" to JsonArray(listOf(actor.userId.j()))) else body
    fun rewrite(op: Operation, temporary: String, real: String): Operation {
        val body = wire.parseToJsonElement(op.body).jsonObject
        val rewritten = if (body.text("intervention_id") == temporary)
            JsonObject(body + ("intervention_id" to real.j())) else body
        return op.copy(dependsOn = if (op.dependsOn == temporary) real else op.dependsOn,
            url = op.url.split('/').joinToString("/") { if (it == temporary) real else it },
            body = rewritten.toString())
    }
    fun acknowledgedId(op: Operation, reply: JsonObject): String? {
        val created = op.tempId?.startsWith("temp_") == true
        val id = reply.text(if (op.kind == "record_time") "intervention_id" else "id")
        return id?.takeIf { it.isNotBlank() && !it.startsWith("temp_") && !reply.text("updated_at").isNullOrBlank() &&
            (created || op.version != null) && (created || id == op.dependsOn) }
    }
    fun acknowledge(op: Operation, reply: JsonObject, pending: List<Operation>): List<Operation> {
        val real = reply.text("id")
        val created = op.tempId?.startsWith("temp_") == true
        val rewritten = pending.map {
            if (created && real != null) rewrite(it, requireNotNull(op.tempId), real) else it
        }
        val id = acknowledgedId(op, reply) ?: return rewritten
        return rewritten.map { next ->
            if (next.id != op.id && next.state == "pending" && next.deletedAt == null &&
                next.dependsOn == id && (created || next.version == op.version))
                next.copy(version = reply.text("updated_at")) else next
        }
    }
    fun blocked(operations: List<Operation>) = operations.filter { it.state == "conflict" && it.deletedAt == null }
        .flatMap { listOfNotNull(it.dependsOn, it.tempId) }.toSet()
    fun owns(actor: Actor, operations: List<Operation>) = operations.all {
        it.userId == actor.userId && it.siteId == actor.siteId && it.scope == actor.scope
    }
}
