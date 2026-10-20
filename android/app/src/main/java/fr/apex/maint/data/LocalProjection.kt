package fr.apex.maint.data

import kotlinx.serialization.json.*

/** Projection d’affichage uniquement ; elle ne recale jamais les préconditions de la file. */
object LocalProjection {
    fun apply(item: JsonObject, operation: Operation): JsonObject {
        val body = wire.parseToJsonElement(operation.body).jsonObject
        val fields = item.toMutableMap()
        fields["server_updated_at"] = if (item.containsKey("server_updated_at"))
            item["server_updated_at"] ?: JsonNull else item["updated_at"] ?: JsonNull
        fields["updated_at"] = operation.occurredAt.j()
        when (operation.kind) {
            "take_intervention" -> {
                fields["status"] = "in_progress".j()
                if (item.text("started_at") == null) fields["started_at"] = operation.occurredAt.j()
                val techs = item.list("technicians")
                if (techs.none { it.text("user_id") == operation.userId }) fields["technicians"] =
                    JsonArray(techs + obj("user_id" to operation.userId.j(), "duration_minutes" to JsonNull))
            }
            "record_time" -> {
                val minutes = requireNotNull(body.text("duration_minutes")?.toIntOrNull()) { "Temps invalide" }
                require(minutes >= 0) { "Le temps doit être positif" }
                val techs = item.list("technicians").toMutableList()
                val previous = techs.find { it.text("user_id") == operation.userId }
                val previousTime = previous?.text("duration_minutes")?.toIntOrNull() ?: 0
                techs.removeAll { it.text("user_id") == operation.userId }
                techs.add(JsonObject((previous ?: obj("user_id" to operation.userId.j())) +
                    ("duration_minutes" to minutes.j())))
                fields["technicians"] = JsonArray(techs)
                fields["duration_minutes"] = ((item.text("duration_minutes")?.toIntOrNull() ?: 0) - previousTime + minutes).j()
            }
            "complete_intervention" -> {
                require(!body.text("solution_report").isNullOrBlank()) { "Le compte rendu est obligatoire" }
                fields["status"] = "completed".j(); fields["completed_at"] = operation.occurredAt.j()
                fields.putAll(body)
            }
            "patch_intervention" -> fields.putAll(body)
        }
        return JsonObject(fields)
    }
    fun replay(server: JsonObject, operations: List<Operation>): JsonObject =
        operations.filter { it.deletedAt == null }.fold(server) { current, op -> apply(current, op) }
}
