package fr.apex.maint.data

import kotlinx.coroutines.runBlocking
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException
import java.util.UUID

class StockConsumptionTest {
    @Test fun `reprise apres coupure et recreation puis nouvelle cle apres succes`() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val api = apiAt(server.url("/").toString(), OkHttpClient.Builder().retryOnConnectionFailure(false).build())
            var saved: String? = null
            fun consumer() = StockConsumption({ saved }, { saved = it }) { api }
            val actor = Actor("u", "s", setOf("stock.movements"))
            val body = obj("part_id" to "p".j(), "location_id" to "l".j(), "quantity" to 2.j())
            server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_AFTER_REQUEST))
            try { consumer().consume(actor, "i", body); fail("Une coupure était attendue") }
            catch (_: IOException) { /* La tentative reste durable. */ }
            val first = server.takeRequest()
            val key = requireNotNull(first.getHeader("Idempotency-Key"))
            UUID.fromString(key)
            assertTrue(requireNotNull(saved).contains(key))
            server.enqueue(MockResponse().setResponseCode(201).setBody("{\"id\":\"consommation\"}"))
            consumer().consume(actor, "i", body)
            val retry = server.takeRequest()
            assertEquals(key, retry.getHeader("Idempotency-Key"))
            assertEquals(first.body.readUtf8(), retry.body.readUtf8())
            assertEquals("{}", saved)
            server.enqueue(MockResponse().setResponseCode(201).setBody("{\"id\":\"autre\"}"))
            consumer().consume(actor, "i", body)
            assertNotEquals(key, server.takeRequest().getHeader("Idempotency-Key"))
        }
    }
    @Test fun `un autre compte site ou corps ne reutilise pas la tentative ambigue`() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val api = apiAt(server.url("/").toString(), OkHttpClient.Builder().retryOnConnectionFailure(false).build())
            var saved: String? = null
            val consumer = StockConsumption({ saved }, { saved = it }) { api }
            val keys = mutableSetOf<String>()
            for ((user, site, quantity) in listOf(Triple("u", "s", 1), Triple("v", "s", 1),
                    Triple("u", "t", 1), Triple("u", "s", 2))) {
                server.enqueue(MockResponse().setResponseCode(503).setBody("{}"))
                try { consumer.consume(Actor(user, site, emptySet()), "i", obj("quantity" to quantity.j())); fail() }
                catch (_: ApiFailure) { /* Réponse serveur ambiguë. */ }
                assertTrue(keys.add(requireNotNull(server.takeRequest().getHeader("Idempotency-Key"))))
            }
        }
    }
}
