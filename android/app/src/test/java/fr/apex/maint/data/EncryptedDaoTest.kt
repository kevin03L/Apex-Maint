package fr.apex.maint.data

import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import javax.crypto.spec.SecretKeySpec

class EncryptedDaoTest {
    private class Memory : MaintDao {
        val cacheRows = mutableListOf<CacheRow>()
        val queue = mutableListOf<Operation>()
        override suspend fun allCache() = cacheRows.toList()
        override suspend fun allOperations() = queue.toList()
        override suspend fun rows(scope: String, collection: String) = cacheRows.filter {
            it.scope == scope && it.collection == collection && it.deletedAt == null }
        override suspend fun row(scope: String, collection: String, id: String) = rows(scope, collection).find { it.id == id }
        override suspend fun cache(rows: List<CacheRow>) { rows.forEach { row ->
            cacheRows.removeAll { it.scope == row.scope && it.collection == row.collection && it.id == row.id }; cacheRows.add(row)
        } }
        override suspend fun operation(op: Operation) { queue.removeAll { it.id == op.id }; queue.add(op) }
        override suspend fun operations(scope: String) = queue.filter { it.scope == scope && it.deletedAt == null }.sortedBy { it.position }
        override fun watchOperations(scope: String) = flowOf(queue.filter { it.scope == scope && it.deletedAt == null })
        override suspend fun nextPosition(scope: String) = (queue.filter { it.scope == scope }.maxOfOrNull { it.position } ?: 0) + 1
        override suspend fun retire(scope: String, collection: String, at: String) {
            cacheRows.replaceAll { if (it.scope == scope && it.collection == collection) it.copy(deletedAt = at) else it }
        }
        override suspend fun purgeCache() { cacheRows.clear() }
        override suspend fun purgeQueue() { queue.clear() }
    }
    private val cipher = PayloadCipher { SecretKeySpec(ByteArray(32) { it.toByte() }, "AES") }
    private fun op() = Operation(scope = "u:s", userId = "u", siteId = "s", kind = "patch_intervention",
        url = "/api/interventions/i", method = "PATCH", body = """{"description":"Confidentiel"}""",
        error = "Refus sensible", state = "conflict", position = 12, version = "v1")

    @Test fun `instantane corps et erreurs sont chiffres et restitues sans changer le protocole`() = runBlocking {
        val raw = Memory(); val dao = EncryptedDao(raw, cipher)
        val row = CacheRow("u:s", "interventions", "i", """{"solution_report":"Confidentiel"}""")
        val op = op()
        dao.cache(listOf(row)); dao.operation(op)
        assertFalse(raw.cacheRows.single().payload.contains("Confidentiel"))
        assertFalse(raw.queue.single().body.contains("Confidentiel"))
        assertFalse(raw.queue.single().error!!.contains("sensible"))
        assertEquals(row, dao.row("u:s", "interventions", "i"))
        assertEquals(op, dao.operations("u:s").single())
        assertEquals(op, dao.watchOperations("u:s").first().single())
        assertEquals(op.headers(), dao.operations("u:s").single().headers())
        assertTrue(dao.operations("autre:s").isEmpty())
    }

    @Test fun `la conversion de l ancien cache conserve aussi les tombstones et la file`() = runBlocking {
        val raw = Memory(); val dao = EncryptedDao(raw, cipher)
        val row = CacheRow("u:s", "interventions", "i", "{}", deletedAt = "date")
        val op = op().copy(deletedAt = "date")
        raw.cache(listOf(row)); raw.operation(op)
        assertTrue(dao.protectLegacy())
        assertTrue(raw.cacheRows.single().payload.startsWith("gcm1:"))
        assertTrue(raw.queue.single().body.startsWith("gcm1:"))
        assertEquals("date", raw.queue.single().deletedAt)
        assertEquals(12L, raw.queue.single().position)
        assertFalse(dao.protectLegacy())
    }

    @Test fun `une reprise et une resolution rechiffrent avec la nouvelle cle d idempotence`() = runBlocking {
        val raw = Memory(); val dao = EncryptedDao(raw, cipher)
        val original = op(); dao.operation(original)
        val retry = dao.operations("u:s").single().copy(id = "nouvelle-cle", state = "pending", error = null)
        dao.operation(original.copy(deletedAt = "date")); dao.operation(retry)
        assertEquals(retry, dao.operations("u:s").single())
        assertNotEquals(raw.queue.first().body, raw.queue.last().body)
    }
}
