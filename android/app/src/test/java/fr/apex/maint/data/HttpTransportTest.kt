package fr.apex.maint.data

import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.IOException
import java.util.concurrent.TimeUnit

class HttpTransportTest {
    @get:Rule val directory = TemporaryFolder()

    @Test fun `multipart reussit apres fermeture de la connexion de la premiere reponse`() {
        MockWebServer().use { server ->
            val client = maintHttpClientBuilder().callTimeout(10, TimeUnit.SECONDS).build()
            server.enqueue(MockResponse().setBody("{}").setSocketPolicy(SocketPolicy.DISCONNECT_AT_END))
            client.newCall(Request.Builder().url(server.url("/api/auth/me")).build()).execute().use {
                assertEquals("{}", it.body!!.string())
            }
            server.enqueue(MockResponse().setResponseCode(201).setBody("{}"))
            val file = directory.newFile().apply { writeBytes(ByteArray(190187)) }
            val body = DocumentUpload.body(file, "i", "f.png", "image/png", false)
            client.newCall(Request.Builder().url(server.url("/api/documents")).post(body).build()).execute().use {
                assertEquals(201, it.code)
            }
            server.takeRequest()
            val upload = server.takeRequest(5, TimeUnit.SECONDS)!!
            assertEquals(0, upload.sequenceNumber)
            assertEquals(body.contentLength(), upload.bodySize)
        }
    }

    @Test fun `rupture pendant ecriture sur connexion reutilisee reprend sur connexion neuve`() {
        MockWebServer().use { server ->
            val client = maintHttpClientBuilder().callTimeout(20, TimeUnit.SECONDS).build()
            server.enqueue(MockResponse().setBody("{}"))
            client.newCall(Request.Builder().url(server.url("/api/auth/me")).build()).execute().use { it.body!!.string() }
            server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_DURING_REQUEST_BODY))
            server.enqueue(MockResponse().setResponseCode(201).setBody("{}"))
            // Dépasse les tampons socket pour observer la rupture avant la fin de writeTo.
            val file = directory.newFile().apply { writeBytes(ByteArray(16 * 1024 * 1024)) }
            val body = DocumentUpload.body(file, "i", "f.png", "image/png", false)
            client.newCall(Request.Builder().url(server.url("/api/documents")).post(body).build()).execute().use {
                assertEquals(201, it.code)
            }
            server.takeRequest()
            val interrupted = server.takeRequest(5, TimeUnit.SECONDS)!!
            val retry = server.takeRequest(5, TimeUnit.SECONDS)!!
            assertEquals(1, interrupted.sequenceNumber)
            assertEquals(0, retry.sequenceNumber)
            assertTrue(interrupted.bodySize < body.contentLength())
            assertEquals(body.contentLength(), retry.bodySize)
        }
    }

    private fun lostReply(path: String, keyed: Boolean, multipart: Boolean) {
        MockWebServer().use { server ->
            val client = maintHttpClientBuilder().callTimeout(10, TimeUnit.SECONDS).build()
            server.enqueue(MockResponse().setBody("{}"))
            client.newCall(Request.Builder().url(server.url("/api/auth/me")).build()).execute().use { it.body!!.string() }
            server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_AFTER_REQUEST))
            server.enqueue(MockResponse().setResponseCode(201).setBody("{}"))
            val body = if (multipart) DocumentUpload.body(directory.newFile().apply { writeText("contenu") },
                "i", "f.png", "image/png", false) else "{}".toRequestBody()
            val request = Request.Builder().url(server.url(path)).post(body)
            if (keyed) request.header("Idempotency-Key", "tentative-stable")
            try {
                client.newCall(request.build()).execute().use { assertTrue(keyed); assertEquals(201, it.code) }
            } catch (e: IOException) { if (keyed) throw e }
            server.takeRequest()
            val first = server.takeRequest(5, TimeUnit.SECONDS)!!
            if (keyed) {
                val retry = server.takeRequest(5, TimeUnit.SECONDS)!!
                assertEquals(first.getHeader("Idempotency-Key"), retry.getHeader("Idempotency-Key"))
                assertEquals(first.body.readUtf8(), retry.body.readUtf8())
                assertEquals(0, retry.sequenceNumber)
            } else assertEquals(2, server.requestCount)
        }
    }
    @Test fun `document entierement ecrit sans reponse nest jamais rejoue`() { lostReply("/api/documents", false, true) }
    @Test fun `post sans cle nest jamais rejoue apres envoi`() { lostReply("/api/auth/login", false, false) }
    @Test fun `operation avec cle reprend corps et cle identiques apres reponse perdue`() {
        lostReply("/api/interventions/i/parts", true, false)
    }
}
