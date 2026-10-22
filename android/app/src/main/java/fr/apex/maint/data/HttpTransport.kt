package fr.apex.maint.data

import okhttp3.ConnectionPool
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.RequestBody
import okio.BufferedSink
import java.util.concurrent.TimeUnit

/** Reprise native sur connexion neuve, sans rejouer une écriture dont le résultat est incertain. */
fun maintHttpClientBuilder(): OkHttpClient.Builder = OkHttpClient.Builder()
    .connectionPool(ConnectionPool(5, 5, TimeUnit.SECONDS))
    .retryOnConnectionFailure(true)
    .addInterceptor { chain ->
        val request = chain.request()
        val body = request.body
        val repeatable = request.method in setOf("GET", "HEAD", "OPTIONS") ||
            !request.header("Idempotency-Key").isNullOrBlank()
        val document = request.method == "POST" && request.url.encodedPath == "/api/documents" && body is MultipartBody
        val guarded = if (body == null || repeatable) body else object : RequestBody() {
            private var complete = false
            private var localFailure = false
            override fun contentType() = body.contentType()
            override fun contentLength() = body.contentLength()
            override fun isOneShot() = body.isOneShot() || !document || complete || localFailure
            override fun writeTo(sink: BufferedSink) {
                try {
                    body.writeTo(sink)
                    // Après remise du multipart complet, une réponse perdue peut cacher un succès.
                    complete = true
                } catch (e: DocumentFileFailure) {
                    localFailure = true
                    throw e
                }
            }
        }
        chain.proceed(if (guarded === body) request else request.newBuilder().method(request.method, guarded).build())
    }
