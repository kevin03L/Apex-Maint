package fr.apex.maint.data

import kotlinx.coroutines.runBlocking
import okhttp3.*
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okio.Buffer
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.concurrent.TimeUnit

class DocumentUploadTest {
    @get:Rule val directory = TemporaryFolder()
    private fun send(photo: Boolean) = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val stored = mutableListOf<Cookie>()
            val client = OkHttpClient.Builder().retryOnConnectionFailure(false).cookieJar(object : CookieJar {
                override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) { stored.addAll(cookies) }
                override fun loadForRequest(url: HttpUrl) = stored.filter { it.matches(url) }
            }).build()
            val api = apiAt(server.url("/").toString(), client)
            server.enqueue(MockResponse().addHeader("Set-Cookie", "apex_maint_session=opaque; HttpOnly; Path=/").setBody("{}"))
            api.post("api/auth/login", obj().requestBody()).checked()
            server.takeRequest()
            val cache = directory.newFolder()
            val lifecycle = TemporaryFiles(cache)
            val temporary = TemporaryFiles(cache)
            val bytes = if (photo) byteArrayOf(-1, -40, -1, -32, 1, 2, 3) else ByteArray(190187) { (it % 127).toByte() }
            val file = if (photo) {
                val capture = temporary.create("jpg").apply { writeBytes(bytes) }
                lifecycle.paused()
                temporary.claimPhoto(capture.name)
            } else temporary.create(external = false).apply { writeBytes(bytes) }
            val name = if (photo) "photo.jpg" else "recette-android.png"
            val mime = if (photo) "image/jpeg" else "image/png"
            val body = DocumentUpload.body(file, "i", name, mime, photo)
            // La longueur est demandée avant la lecture différée par OkHttp.
            val length = body.contentLength()
            lifecycle.resumed()
            assertTrue(file.exists())
            server.enqueue(MockResponse().setResponseCode(201).setBody("""{"id":"d"}"""))
            try {
                assertEquals("d", api.post("api/documents", body).checked().objectOrEmpty().text("id"))
                assertTrue(file.exists())
                val request = requireNotNull(server.takeRequest(5, TimeUnit.SECONDS))
                assertEquals("/api/documents", request.path)
                assertEquals("POST", request.method)
                assertEquals("apex_maint_session=opaque", request.getHeader("Cookie"))
                assertTrue(request.getHeader("Content-Type")!!.startsWith("multipart/form-data; boundary="))
                assertEquals(length.toString(), request.getHeader("Content-Length"))
                assertEquals(length, request.bodySize)
                val raw = request.body.readByteArray()
                val text = raw.toString(Charsets.ISO_8859_1)
                for ((field, value) in listOf("entity_type" to "intervention", "entity_id" to "i", "title" to name,
                        "category" to if (photo) "photo" else "autre")) {
                    assertTrue(text.contains("name=\"$field\"\r\n"))
                    assertTrue(text.contains("\r\n\r\n$value\r\n"))
                }
                assertTrue(text.contains("name=\"file\"; filename=\"$name\""))
                assertTrue(text.contains("Content-Type: $mime"))
                val start = text.indexOf("\r\n\r\n", text.indexOf("name=\"file\"")) + 4
                assertArrayEquals(bytes, raw.copyOfRange(start, start + bytes.size))
            } finally { file.delete() }
            assertFalse(file.exists())
        }
    }
    @Test fun `photo privee envoyee apres purge de reprise avec cookie et multipart exact`() { send(true) }
    @Test fun `fichier PNG de 190187 octets envoye apres reprise avec cookie et taille exacte`() { send(false) }
    @Test fun `fichier disparu apres calcul de longueur est une erreur locale`() {
        val file = directory.newFile().apply { writeText("photo") }
        val body = DocumentUpload.body(file, "i", "photo.jpg", "image/jpeg", true)
        body.contentLength()
        file.delete()
        try { body.writeTo(Buffer()); fail() }
        catch (e: DocumentFileFailure) { assertTrue(e.cause is java.io.FileNotFoundException) }
    }
    @Test fun `refus serveur conserve son statut documentaire`() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val file = directory.newFile().apply { writeText("contenu") }
            val api = apiAt(server.url("/").toString(), OkHttpClient())
            server.enqueue(MockResponse().setResponseCode(415).setBody("""{"message":"Format refusé"}"""))
            try { api.post("api/documents", DocumentUpload.body(file, "i", "f.png", "image/png", false)).checked(); fail() }
            catch (e: ApiFailure) { assertEquals(415, e.status); assertEquals("Format refusé", e.message) }
        }
    }
}
