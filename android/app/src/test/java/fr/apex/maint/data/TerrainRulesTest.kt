package fr.apex.maint.data

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class TerrainRulesTest {
    private fun item(minutes: JsonElement = JsonNull, status: String = "in_progress") = obj(
        "status" to status.j(), "technicians" to JsonArray(listOf(obj("user_id" to "u".j(), "duration_minutes" to minutes))))

    @Test fun `un temps non renseigne differe d un temps explicitement nul`() {
        assertNotNull(TerrainRules.completionProblem(item(), "Travaux terminés"))
        assertNull(TerrainRules.completionProblem(item(0.j()), "Travaux terminés"))
        assertNull(TerrainRules.completionProblem(item(30.j()), "Travaux terminés"))
    }
    @Test fun `le rapport et le statut conditionnent la cloture`() {
        assertNotNull(TerrainRules.completionProblem(item(30.j()), "  "))
        assertNotNull(TerrainRules.completionProblem(item(30.j(), "completed"), "Terminé"))
        assertNotNull(TerrainRules.completionProblem(item(30.j(), "cancelled"), "Terminé"))
        assertNull(TerrainRules.completionProblem(item(30.j(), "open"), "Terminé"))
    }
    @Test fun `les sous interventions actives doivent etre terminees`() {
        for (status in listOf("open", "in_progress")) assertNotNull(TerrainRules.completionProblem(item(30.j()),
            "Terminé", listOf(obj("status" to status.j()))))
        assertNull(TerrainRules.completionProblem(item(30.j()), "Terminé", listOf(obj("status" to "completed".j()))))
    }
    @Test fun `les limites des textes suivent le contrat en caracteres Unicode`() {
        assertNull(TerrainRules.inputProblem(obj("title" to "é".repeat(128).j(), "description" to "a".repeat(4096).j())))
        assertNotNull(TerrainRules.inputProblem(obj("title" to "a".repeat(129).j())))
        assertNotNull(TerrainRules.inputProblem(obj("description" to "a".repeat(4097).j())))
        assertNotNull(TerrainRules.inputProblem(obj("title" to "Ligne\ninterdite".j())))
        assertNotNull(TerrainRules.inputProblem(obj("solution_report" to "a".repeat(16385).j())))
    }
    @Test fun `la limite du corps est verifiee en octets apres serialisation`() {
        val body = obj("solution_report" to "\u0001".repeat(16384).j())
        assertTrue(TerrainRules.inputProblem(body).orEmpty().contains("64 Kio"))
    }
}
