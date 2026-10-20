package fr.apex.maint.data

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.*
import okhttp3.*
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.*
import org.junit.Test

class ApiTest {
    @Test fun `session et permissions acceptent les champs supplementaires et null`() {
        val session = wire.decodeFromString<Session>("""{
            "user":{"id":"u","site_id":null,"display_name":"Élodie","role":"admin",
                "must_change_password":true,"username":"elodie","active":true},
            "permissions":["interventions.view","documents.create"],"site":null,"must_change_password":true
        }""")
        assertTrue(session.must_change_password)
        assertNull(session.user.site_id)
        assertTrue("documents.create" in session.permissions)
        assertEquals(session, wire.decodeFromString<Session>(wire.encodeToString(session)))
    }
    @Test fun `instantane conserve les collections et les tombstones`() {
        val snapshot = wire.decodeFromString<Snapshot>("""{
            "generated_at":"2026-10-18T12:00:00.000Z","full":false,
            "equipments":[],"production_lines":[],"counters":[],"parts":[],"users":[],
            "interventions":[{"id":"i","site_id":"s","description":null,"deleted_at":"date"}],
            "technicians":[],"intervention_ids":[]
        }""")
        assertFalse(snapshot.full)
        assertEquals("date", snapshot.interventions.single().text("deleted_at"))
        assertEquals(JsonNull, snapshot.interventions.single()["description"])
        assertTrue(snapshot.technicians.isEmpty())
        assertEquals(snapshot, wire.decodeFromString<Snapshot>(wire.encodeToString(snapshot)))
    }
    @Test fun `connexion cookie HttpOnly et PATCH avec preconditions`() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val stored = mutableListOf<Cookie>()
            val client = OkHttpClient.Builder().cookieJar(object : CookieJar {
                override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) { stored.addAll(cookies) }
                override fun loadForRequest(url: HttpUrl) = stored.filter { it.matches(url) }
            }).build()
            val api = apiAt(server.url("/").toString(), client)
            server.enqueue(MockResponse().setHeader("Content-Type", "application/json")
                .addHeader("Set-Cookie", "apex_maint_session=opaque; HttpOnly; SameSite=Strict; Path=/")
                .setBody("""{"user":{"id":"u"}}"""))
            api.post("api/auth/login", obj("username" to "moi".j(), "password" to "éphémère".j()).requestBody()).checked()
            val login = server.takeRequest()
            assertEquals("/api/auth/login", login.path)
            assertEquals("moi", wire.parseToJsonElement(login.body.readUtf8()).jsonObject.text("username"))
            assertTrue(stored.single().httpOnly)
            server.enqueue(MockResponse().setBody("""{"id":"i","updated_at":"v2"}"""))
            val op = Operation(scope = "u:s", userId = "u", siteId = "s", kind = "patch_intervention",
                url = "/api/interventions/i", method = "PATCH", body = "{}", version = "v1")
            api.patch("api/interventions/i", obj("description" to JsonNull).requestBody(), op.headers()).checked()
            val patch = server.takeRequest()
            assertEquals("PATCH", patch.method)
            assertEquals("v1", patch.getHeader("If-Unmodified-Since"))
            assertEquals(op.id, patch.getHeader("Idempotency-Key"))
            assertEquals(op.occurredAt, patch.getHeader("X-Occurred-At"))
            assertEquals("apex_maint_session=opaque", patch.getHeader("Cookie"))
            assertEquals(JsonNull, wire.parseToJsonElement(patch.body.readUtf8()).jsonObject["description"])
        }
    }
    @Test fun `toutes les erreurs HTTP gardent le message serveur`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val api = apiAt(server.url("/").toString(), OkHttpClient())
            for (status in listOf(400, 401, 403, 409, 412, 422, 503)) {
                server.enqueue(MockResponse().setResponseCode(status).setBody("""{"error":true,"message":"Refus précis"}"""))
                try { api.get("api/auth/me").checked(); fail() }
                catch (e: ApiFailure) {
                    assertEquals(status, e.status); assertEquals("Refus précis", e.message)
                    assertEquals(when (status) { 401 -> HttpDisposition.SESSION; 503 -> HttpDisposition.RETRY
                        else -> HttpDisposition.CONFLICT }, disposition(e.status))
                }
            }
        }
    }
    @Test fun `mot de passe impose et reponse sans corps`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val api = apiAt(server.url("/").toString(), OkHttpClient())
            server.enqueue(MockResponse().setResponseCode(403).setBody("""{"error":true,
                "message":"Changez votre mot de passe","code":"password_change_required"}"""))
            try { api.get("api/offline/snapshot").checked(); fail() }
            catch (e: ApiFailure) { assertEquals("password_change_required", e.code) }
            server.enqueue(MockResponse().setResponseCode(204))
            assertNull(api.post("api/auth/logout", obj().requestBody()).checked())
        }
    }
    @Test fun `les erreurs documentaires sans JSON ont une explication francaise`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val api = apiAt(server.url("/").toString(), OkHttpClient())
            for ((status, word) in listOf(410 to "stockage", 413 to "taille", 415 to "type")) {
                server.enqueue(MockResponse().setResponseCode(status).setBody(""))
                try { api.get("api/documents/d").checked(); fail() }
                catch (e: ApiFailure) { assertEquals(status, e.status); assertTrue(e.message.contains(word)) }
            }
        }
    }
    @Test fun `une operation Room se serialise sans perdre sa position ni precondition`() {
        val operation = Operation(scope = "u:s", userId = "u", siteId = "s", kind = "record_time",
            url = "/api/interventions/temp_1/technicians/u/time", method = "PUT",
            body = """{"duration_minutes":25}""", dependsOn = "temp_1", position = 42, version = null)
        assertEquals(operation, wire.decodeFromString<Operation>(wire.encodeToString(operation)))
    }
    @Test fun `prise temps et cloture utilisent les corps et routes du serveur`() = runBlocking {
        MockWebServer().use { server ->
            server.start(); val api = apiAt(server.url("/").toString(), OkHttpClient())
            val actor = Actor("u", "s", emptySet())
            fun headers(version: String) = Operation(scope = "u:s", userId = "u", siteId = "s", kind = "take_intervention",
                url = "/api/interventions/i/take", method = "POST", body = "{}", version = version).headers()
            server.enqueue(MockResponse().setBody("""{"id":"i","updated_at":"v2"}"""))
            api.post("api/interventions/i/take", QueueProtocol.bodyForActor(actor, "take_intervention", obj()).requestBody(), headers("version-exacte")).checked()
            val take = server.takeRequest()
            assertEquals("/api/interventions/i/take", take.path)
            assertEquals("POST", take.method)
            assertEquals(listOf("u".j()), wire.parseToJsonElement(take.body.readUtf8()).jsonObject["technician_ids"]!!.jsonArray.toList())
            server.enqueue(MockResponse().setBody("""{"user_id":"u","intervention_id":"i","duration_minutes":0,"updated_at":"v3"}"""))
            val time = api.put("api/interventions/i/technicians/u/time", obj("duration_minutes" to 0.j()).requestBody(), headers("v2")).checked().objectOrEmpty()
            val timed = server.takeRequest()
            assertEquals("/api/interventions/i/technicians/u/time", timed.path)
            assertEquals("PUT", timed.method)
            assertEquals(0.j(), wire.parseToJsonElement(timed.body.readUtf8()).jsonObject["duration_minutes"])
            assertEquals("i", time.text("intervention_id"))
            server.enqueue(MockResponse().setBody("""{"id":"i","status":"completed","updated_at":"v4"}"""))
            api.post("api/interventions/i/complete", obj("solution_report" to "Réparé".j()).requestBody(), headers("v3")).checked()
            val complete = server.takeRequest()
            assertEquals("/api/interventions/i/complete", complete.path)
            assertEquals("Réparé", wire.parseToJsonElement(complete.body.readUtf8()).jsonObject.text("solution_report"))
            assertEquals(listOf("version-exacte", "v2", "v3"), listOf(take, timed, complete).map { it.getHeader("If-Unmodified-Since") })
            assertEquals(3, listOf(take, timed, complete).map { it.getHeader("Idempotency-Key") }.toSet().size)
            for (request in listOf(take, timed, complete)) {
                assertNotNull(request.getHeader("Idempotency-Key"))
                assertNotNull(request.getHeader("X-Occurred-At"))
            }
        }
    }
}
