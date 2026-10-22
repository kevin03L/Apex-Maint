package fr.apex.maint.data

import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonObject
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.*
import org.junit.Test
import javax.crypto.spec.SecretKeySpec

class ConflictResolverTest {
    private class Memory : MaintDao {
        val rows = mutableListOf<CacheRow>()
        val queue = mutableListOf<Operation>()
        override suspend fun allCache() = rows.toList()
        override suspend fun allOperations() = queue.toList()
        override suspend fun rows(scope: String, collection: String) = rows.filter {
            it.scope == scope && it.collection == collection && it.deletedAt == null }
        override suspend fun row(scope: String, collection: String, id: String) = rows(scope, collection).find { it.id == id }
        override suspend fun cache(rows: List<CacheRow>) { rows.forEach { row ->
            this.rows.removeAll { it.scope == row.scope && it.collection == row.collection && it.id == row.id }; this.rows.add(row)
        } }
        override suspend fun operation(op: Operation) { queue.removeAll { it.id == op.id }; queue.add(op) }
        override suspend fun operations(scope: String) = queue.filter { it.scope == scope && it.deletedAt == null }.sortedBy { it.position }
        override fun watchOperations(scope: String) = flowOf(queue.filter { it.scope == scope && it.deletedAt == null })
        override suspend fun nextPosition(scope: String) = 1L
        override suspend fun retire(scope: String, collection: String, at: String) {
            rows.replaceAll { if (it.scope == scope && it.collection == collection) it.copy(deletedAt = at) else it }
        }
        override suspend fun purgeCache() { rows.clear() }
        override suspend fun purgeQueue() { queue.clear() }
    }
    private val actor = Actor("u", "s", emptySet())
    private val cipher = PayloadCipher { SecretKeySpec(ByteArray(32) { it.toByte() }, "AES") }
    private fun op() = Operation(scope = actor.scope, userId = "u", siteId = "s", dependsOn = "i", kind = "patch_intervention",
        url = "/api/interventions/i", method = "PATCH", body = """{"description":"locale abandonnée"}""",
        position = 7, version = "v1", state = "conflict", errorStatus = 412)
    private fun remote(version: String = "v2") = MockResponse().setBody(
        """{"id":"i","site_id":"s","description":"distante","solution_report":"rapport distant","updated_at":"$version"}""")

    @Test fun `abandon recharge le serveur rejoue les autres actions et invalide le brouillon chiffre`() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val raw = Memory(); val dao = EncryptedDao(raw, cipher); val conflict = op()
            dao.operation(conflict)
            dao.operation(conflict.copy(id = "autre-action", state = "pending", position = 8,
                body = """{"solution_report":"autre action conservée"}"""))
            dao.cache(listOf(CacheRow(actor.scope, "server_interventions", "i",
                """{"id":"i","site_id":"s","description":"ancienne base"}""")))
            val oldContext = draftContext("serveur", actor.scope, "i:description", emptyMap())
            val saved = cipher.encrypt(oldContext, "locale abandonnée")
            val otherContext = draftContext("serveur", actor.scope, "autre:description", emptyMap())
            val otherSaved = cipher.encrypt(otherContext, "autre brouillon")
            server.enqueue(remote())
            var transactions = 0
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            ConflictResolver(dao, { api }) { work -> transactions++; work() }.resolve(actor, conflict.id, false, null)
            assertEquals(1, transactions)
            assertEquals("/api/interventions/i", server.takeRequest().path)
            val projected = wire.parseToJsonElement(dao.row(actor.scope, "interventions", "i")!!.payload).jsonObject
            assertEquals("distante", projected.text("description"))
            assertEquals("autre action conservée", projected.text("solution_report"))
            assertEquals("v2", projected.text("server_updated_at"))
            assertEquals("autre-action", dao.operations(actor.scope).single().id)
            assertEquals("abandoned", raw.queue.first { it.id == conflict.id }.state)
            val revision = wire.parseToJsonElement(dao.row(actor.scope, "draft_revisions", "i")!!.payload).jsonObject.text("revision")!!
            val revisions = mapOf("i" to revision)
            try { cipher.decrypt(draftContext("serveur", actor.scope, "i:description", revisions), saved); fail() }
            catch (_: java.security.GeneralSecurityException) { }
            assertEquals("autre brouillon", cipher.decrypt(draftContext("serveur", actor.scope, "autre:description", revisions), otherSaved))
            assertNull(dao.row("autre:s", "draft_revisions", "i"))
        }
    }
    @Test fun `reessai garde ordre et projection voulue renouvelle cle et brouillons`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val raw = Memory(); val dao = EncryptedDao(raw, cipher); val conflict = op()
            dao.operation(conflict); server.enqueue(remote())
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            ConflictResolver(dao, { api }) { it() }.resolve(actor, conflict.id, true, "v2")
            val retry = dao.operations(actor.scope).single()
            assertNotEquals(conflict.id, retry.id); assertEquals(7L, retry.position); assertEquals("v2", retry.version)
            assertEquals("pending", retry.state); assertNull(retry.errorStatus)
            assertEquals("locale abandonnée", wire.parseToJsonElement(dao.row(actor.scope, "interventions", "i")!!.payload).jsonObject.text("description"))
            assertNotNull(dao.row(actor.scope, "draft_revisions", "i"))
        }
    }
    @Test fun `reessai refuse une nouvelle version sans toucher file projection ou brouillons`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val raw = Memory(); val dao = EncryptedDao(raw, cipher); val conflict = op()
            dao.operation(conflict); server.enqueue(remote("v3"))
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            try { ConflictResolver(dao, { api }) { fail("Aucune transaction attendue") }.resolve(actor, conflict.id, true, "v2"); fail() }
            catch (_: IllegalArgumentException) { }
            assertEquals(conflict, dao.operations(actor.scope).single())
            assertNull(dao.row(actor.scope, "draft_revisions", "i"))
        }
    }
    @Test fun `abandon conserve file et brouillons si le rechargement serveur echoue`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val raw = Memory(); val conflict = op()
            raw.operation(conflict)
            server.enqueue(MockResponse().setResponseCode(503).setBody("{}"))
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            try { ConflictResolver(raw, { api }) { fail("Aucune transaction attendue") }.resolve(actor, conflict.id, false, null); fail() }
            catch (e: ApiFailure) { assertEquals(503, e.status) }
            assertEquals(conflict, raw.operations(actor.scope).single())
            assertTrue(raw.rows.isEmpty())
        }
    }
    @Test fun `abandon refuse une entite dun autre site sans mutation`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val raw = Memory(); val conflict = op()
            raw.operation(conflict)
            server.enqueue(MockResponse().setBody("""{"id":"i","site_id":"autre","description":"distante"}"""))
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            try { ConflictResolver(raw, { api }) { fail("Aucune transaction attendue") }.resolve(actor, conflict.id, false, null); fail() }
            catch (_: IllegalArgumentException) { }
            assertEquals(conflict, raw.operations(actor.scope).single())
            assertTrue(raw.rows.isEmpty())
        }
    }
    @Test fun `abandon dune creation retire ses dependances sans requete serveur`() = runBlocking {
        val raw = Memory(); val conflict = op().copy(tempId = "temp_i", dependsOn = null, kind = "create_intervention")
        raw.operation(conflict)
        raw.operation(op().copy(id = "dependance", dependsOn = "temp_i", state = "pending"))
        raw.cache(listOf(CacheRow(actor.scope, "interventions", "temp_i", "{}")))
        ConflictResolver(raw, { error("Pas de réseau attendu") }) { it() }.resolve(actor, conflict.id, false, null)
        assertTrue(raw.operations(actor.scope).isEmpty())
        assertNull(raw.row(actor.scope, "interventions", "temp_i"))
        assertNotNull(raw.row(actor.scope, "draft_revisions", "temp_i"))
    }
}
