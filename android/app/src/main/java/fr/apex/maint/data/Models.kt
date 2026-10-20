package fr.apex.maint.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*

val wire = Json { ignoreUnknownKeys = true; encodeDefaults = true }
fun JsonObject.text(key: String): String? = (this[key] as? JsonPrimitive)?.contentOrNull
fun JsonObject.list(key: String): List<JsonObject> = (this[key] as? JsonArray)?.mapNotNull { it as? JsonObject }.orEmpty()
fun obj(vararg fields: Pair<String, JsonElement?>) = JsonObject(fields.associate { it.first to (it.second ?: JsonNull) })
fun String.j() = JsonPrimitive(this)
fun Int.j() = JsonPrimitive(this)

@Serializable data class User(val id: String, val site_id: String? = null, val display_name: String,
    val role: String, val must_change_password: Boolean = false)
@Serializable data class Site(val id: String, val name: String)
@Serializable data class Session(val user: User, val permissions: Set<String> = emptySet(),
    val site: Site? = null, val must_change_password: Boolean = false)
@Serializable data class Snapshot(val generated_at: String, val full: Boolean,
    val equipments: List<JsonObject> = emptyList(), val production_lines: List<JsonObject> = emptyList(),
    val counters: List<JsonObject> = emptyList(), val parts: List<JsonObject> = emptyList(),
    val users: List<JsonObject> = emptyList(), val interventions: List<JsonObject> = emptyList(),
    val technicians: List<JsonObject> = emptyList(), val intervention_ids: List<String> = emptyList())

data class Actor(val userId: String, val siteId: String, val permissions: Set<String>, val isAdmin: Boolean = false) {
    val scope get() = "$userId:$siteId"
    fun can(permission: String) = isAdmin || permission in permissions
}
enum class HttpDisposition { SESSION, CONFLICT, RETRY }
fun disposition(code: Int) = when (code) {
    401 -> HttpDisposition.SESSION
    400, 403, 409, 412, 422 -> HttpDisposition.CONFLICT
    else -> HttpDisposition.RETRY
}
class ApiFailure(val status: Int, override val message: String, val code: String? = null) : Exception(message)
