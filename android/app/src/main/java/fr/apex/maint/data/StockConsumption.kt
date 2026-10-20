package fr.apex.maint.data

import kotlinx.serialization.json.*
import java.util.UUID

/** Appelé sous le verrou de session ; read/write utilisent le stockage chiffré durable. */
class StockConsumption(private val read: () -> String?, private val write: (String?) -> Unit,
    private val api: () -> MaintApi) {
    suspend fun consume(actor: Actor, interventionId: String, body: JsonObject) {
        val path = "api/interventions/${encode(interventionId)}/parts"
        val identity = obj("scope" to actor.scope.j(), "route" to path.j(), "body" to body).toString()
        val pending = read()?.let { wire.parseToJsonElement(it).jsonObject.toMutableMap() } ?: mutableMapOf()
        val key = pending[identity]?.jsonPrimitive?.content ?: UUID.randomUUID().toString().also {
            pending[identity] = it.j()
            write(JsonObject(pending).toString())
        }
        api().post(path, body.requestBody(), mapOf("Idempotency-Key" to key)).checked()
        pending.remove(identity)
        write(JsonObject(pending).toString())
    }
}
