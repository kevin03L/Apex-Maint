package fr.apex.maint.data

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

class QueueProtocolTest {
    private val actor = Actor("moi", "site", setOf("interventions.edit"))
    private fun op(kind: String = "patch_intervention", target: String = "int", version: String? = "v1") =
        Operation(scope = actor.scope, userId = actor.userId, siteId = actor.siteId, kind = kind,
            url = "/api/interventions/$target", method = "PATCH", body = "{}", dependsOn = target, version = version)
    private fun reply(version: String = "v2", id: String = "int") = obj("id" to id.j(), "updated_at" to version.j())

    private class Memory(initial: List<Operation>) {
        var rows = initial
        val sent = mutableListOf<Operation>()
        suspend fun run(actor: Actor, transport: suspend (Operation) -> JsonObject): Boolean =
            QueueRunner.execute(actor, { rows }, send = { sent += it; transport(it) }, acknowledge = { op, body ->
                rows = QueueProtocol.acknowledge(op, body, rows).filter { it.id != op.id }
            }, conflict = { op, error -> rows = rows.map {
                if (it.id == op.id) it.copy(state = "conflict", error = error.message, errorStatus = error.status) else it
            } })
    }
    @Test fun `deux PATCH chainent la version et conservent les cles`() = runBlocking {
        val first = op(); val second = op()
        val memory = Memory(listOf(first, second))
        memory.run(actor) { if (it.id == first.id) reply("v2") else reply("v3") }
        assertEquals(listOf("v1", "v2"), memory.sent.map { it.version })
        assertEquals(listOf(first.id, second.id), memory.sent.map { it.headers()["Idempotency-Key"] })
        assertTrue(memory.rows.isEmpty())
    }
    @Test fun `la coupure conserve la precondition acquittee pour la reprise`() = runBlocking {
        val first = op(); val second = op()
        val memory = Memory(listOf(first, second))
        try { memory.run(actor) { if (it.id == first.id) reply() else throw IOException("Réseau coupé") }; fail() }
        catch (_: IOException) { }
        assertEquals("v2", memory.rows.single().version)
        val resumed = Memory(memory.rows)
        resumed.run(actor) { reply("v3") }
        assertEquals(second.id, resumed.sent.single().id)
        assertEquals("v2", resumed.sent.single().version)
    }
    @Test fun `la creation initialise deux PATCH et reecrit le corps et URL`() = runBlocking {
        val creation = op("create_intervention", version = null).copy(tempId = "temp_1", dependsOn = null,
            url = "/api/interventions", method = "POST")
        val first = op(target = "temp_1", version = null).copy(body = """{"intervention_id":"temp_1","description":"temp_1"}""")
        val second = op(target = "temp_1", version = null)
        val memory = Memory(listOf(creation, first, second))
        memory.run(actor) { when (it.id) {
            creation.id -> reply("v1", "reel")
            first.id -> reply("v2", "reel")
            else -> reply("v3", "reel")
        } }
        assertEquals(listOf(null, "v1", "v2"), memory.sent.map { it.version })
        assertEquals("/api/interventions/reel", memory.sent[1].url)
        assertEquals("reel", wire.parseToJsonElement(memory.sent[1].body).jsonObject.text("intervention_id"))
        assertEquals("temp_1", wire.parseToJsonElement(memory.sent[1].body).jsonObject.text("description"))
    }
    @Test fun `prise temps cloture et PATCH chainent seulement la meme version`() = runBlocking {
        for (kind in listOf("take_intervention", "record_time", "complete_intervention")) {
            val first = op(kind); val second = op(); val independentVersion = op(version = "autre")
            val memory = Memory(listOf(first, second, independentVersion))
            memory.run(actor) { current ->
                if (current.id == first.id && kind == "record_time") obj("intervention_id" to "int".j(), "updated_at" to "v2".j())
                else reply(if (current.id == first.id) "v2" else "v3")
            }
            assertEquals(listOf("v1", "v2", "autre"), memory.sent.map { it.version })
        }
    }
    @Test fun `un acquittement sans condition ne recale pas le PATCH suivant`() = runBlocking {
        val legacy = op(version = null); val next = op()
        val memory = Memory(listOf(legacy, next))
        memory.run(actor) { reply() }
        assertEquals("v1", memory.sent[1].version)
        assertNull(QueueProtocol.acknowledgedId(legacy, reply()))
    }
    @Test fun `une reponse pour une autre intervention ne recale rien`() {
        val first = op(); val next = op()
        assertEquals("v1", QueueProtocol.acknowledge(first, reply(id = "autre"), listOf(next)).single().version)
    }
    @Test fun `conflits metier bloquent uniquement les dependances`() = runBlocking {
        for (code in listOf(400, 403, 409, 412, 422)) {
            val first = op(); val dependent = op(); val separate = op(target = "autre")
            val memory = Memory(listOf(first, dependent, separate))
            assertTrue(memory.run(actor) { if (it.id == first.id) throw ApiFailure(code, "Refus") else reply(id = "autre") })
            assertEquals(listOf(first.id, separate.id), memory.sent.map { it.id })
            assertEquals("conflict", memory.rows.first().state)
            assertEquals(code, memory.rows.first().errorStatus)
            val resumed = Memory(memory.rows)
            resumed.run(actor) { fail("La dépendance ne doit pas partir"); reply() }
            assertTrue(resumed.sent.isEmpty())
        }
    }
    @Test fun `une concurrence reelle produit 412 sans masquer la version`() = runBlocking {
        val memory = Memory(listOf(op(), op()))
        memory.run(actor) { assertEquals("v1", it.version); throw ApiFailure(412, "Version distante v9") }
        assertEquals(1, memory.sent.size)
        assertEquals("v1", memory.rows.last().version)
    }
    @Test fun `creation refusee bloque toutes les references temporaires`() = runBlocking {
        val creation = op().copy(tempId = "temp_1", dependsOn = null)
        val memory = Memory(listOf(creation, op(target = "temp_1", version = null)))
        memory.run(actor) { throw ApiFailure(422, "Refus") }
        assertEquals(1, memory.sent.size)
        assertEquals(setOf("temp_1"), QueueProtocol.blocked(memory.rows))
    }
    @Test fun `une session expiree preserve la file et arrete tout`() = runBlocking {
        val original = listOf(op(), op(target = "autre")); val memory = Memory(original)
        try { memory.run(actor) { throw ApiFailure(401, "Session expirée") }; fail() }
        catch (e: ApiFailure) { assertEquals(401, e.status) }
        assertEquals(original, memory.rows); assertEquals(1, memory.sent.size)
    }
    @Test fun `une erreur serveur preserve la cle pour reessayer`() = runBlocking {
        val original = listOf(op(), op()); val memory = Memory(original)
        assertFalse(memory.run(actor) { throw ApiFailure(503, "Indisponible") })
        assertEquals(original, memory.rows)
    }
    @Test fun `aucun envoi sous un autre utilisateur ou site`() = runBlocking {
        for (other in listOf(actor.copy(userId = "autre"), actor.copy(siteId = "autre"))) {
            val memory = Memory(listOf(op()))
            try { memory.run(other) { fail("Identité différente"); reply() }; fail() }
            catch (_: IllegalArgumentException) { }
            assertTrue(memory.sent.isEmpty())
        }
    }
    @Test fun `une file vide apres purge ne transmet rien`() = runBlocking {
        val memory = Memory(emptyList())
        assertTrue(memory.run(actor) { fail("Cache purgé"); reply() })
        assertTrue(memory.sent.isEmpty())
    }
    @Test fun `les trois entetes sont exactement ceux du web`() {
        val op = op()
        assertEquals(mapOf("Idempotency-Key" to op.id, "X-Occurred-At" to op.occurredAt,
            "If-Unmodified-Since" to "v1"), op.headers())
        assertFalse(op.copy(version = null).headers().containsKey("If-Unmodified-Since"))
    }
    @Test fun `une reecriture respecte les segments URL complets`() {
        val changed = QueueProtocol.rewrite(op(target = "temp_1_suffix"), "temp_1", "reel")
        assertEquals("/api/interventions/temp_1_suffix", changed.url)
    }
    @Test fun `une reponse incomplete ne perd pas une operation conditionnee`() = runBlocking {
        val original = listOf(op())
        val memory = Memory(original)
        try { memory.run(actor) { obj() }; fail() }
        catch (_: IOException) { }
        assertEquals(original, memory.rows)
    }
}
