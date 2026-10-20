package fr.apex.maint.data

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class LocalProjectionTest {
    private fun operation(kind: String, body: JsonObject = obj()) = Operation(
        scope = "u:s", userId = "u", siteId = "s", kind = kind, url = "/api/interventions/i",
        method = "POST", body = body.toString(), dependsOn = "i", version = "serveur-v1",
        occurredAt = "2026-10-20T10:00:00.000Z")
    private fun initial() = obj("id" to "i".j(), "site_id" to "s".j(), "status" to "open".j(),
        "updated_at" to "serveur-v1".j(), "description" to "Ancienne description".j(),
        "duration_minutes" to 20.j(), "technicians" to JsonArray(listOf(
            obj("user_id" to "autre".j(), "duration_minutes" to 20.j()))))

    @Test fun `prise temps et cloture conservent la version serveur initiale`() {
        val taken = LocalProjection.apply(initial(), operation("take_intervention"))
        assertEquals("in_progress", taken.text("status"))
        assertEquals("2026-10-20T10:00:00.000Z", taken.text("started_at"))
        val timed = LocalProjection.apply(taken, operation("record_time", obj("duration_minutes" to 30.j())))
        val completed = LocalProjection.apply(timed, operation("complete_intervention", obj("solution_report" to "Réparé".j())))
        assertEquals("completed", completed.text("status"))
        assertEquals("Réparé", completed.text("solution_report"))
        assertEquals("50", completed.text("duration_minutes"))
        assertEquals("serveur-v1", completed.text("server_updated_at"))
        assertNotNull(completed.text("completed_at"))
    }
    @Test fun `la saisie remplace mon total sans additionner deux fois`() {
        val first = LocalProjection.apply(initial(), operation("record_time", obj("duration_minutes" to 30.j())))
        val second = LocalProjection.apply(first, operation("record_time", obj("duration_minutes" to 45.j())))
        assertEquals("65", second.text("duration_minutes"))
        assertEquals(2, second.list("technicians").size)
        assertEquals("45", second.list("technicians").first { it.text("user_id") == "u" }.text("duration_minutes"))
    }
    @Test fun `un PATCH peut mettre la description a null sans changer le rapport`() {
        val initial = JsonObject(initial() + ("solution_report" to "Rapport".j()))
        val result = LocalProjection.apply(initial, operation("patch_intervention", obj("description" to JsonNull)))
        assertEquals(JsonNull, result["description"])
        assertEquals("Rapport", result.text("solution_report"))
        assertEquals("serveur-v1", result.text("server_updated_at"))
    }
    @Test fun `la creation provisoire conserve une precondition nulle`() {
        val initial = JsonObject(initial() + ("server_updated_at" to JsonNull))
        val result = LocalProjection.apply(initial, operation("patch_intervention", obj("description" to "Locale".j())))
        assertEquals(JsonNull, result["server_updated_at"])
    }
    @Test fun `abandonner une modification reconstruit le serveur et les operations restantes`() {
        val server = initial()
        val discarded = operation("patch_intervention", obj("description" to "À abandonner".j())).copy(deletedAt = "date")
        val keep = operation("record_time", obj("duration_minutes" to 15.j()))
        val restored = LocalProjection.replay(server, listOf(discarded, keep))
        assertEquals("Ancienne description", restored.text("description"))
        assertEquals("35", restored.text("duration_minutes"))
    }
    @Test(expected = IllegalArgumentException::class) fun `la cloture exige un compte rendu`() {
        LocalProjection.apply(initial(), operation("complete_intervention"))
    }
}
