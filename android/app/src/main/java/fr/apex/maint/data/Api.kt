package fr.apex.maint.data

import kotlinx.serialization.json.*
import okhttp3.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import retrofit2.Response
import retrofit2.Retrofit
import retrofit2.converter.kotlinx.serialization.asConverterFactory
import retrofit2.http.*

interface MaintApi {
    @GET suspend fun get(@Url path: String): Response<JsonElement>
    @POST suspend fun post(@Url path: String, @Body body: RequestBody,
        @HeaderMap headers: Map<String, String> = emptyMap()): Response<JsonElement>
    @PUT suspend fun put(@Url path: String, @Body body: RequestBody,
        @HeaderMap headers: Map<String, String> = emptyMap()): Response<JsonElement>
    @PATCH suspend fun patch(@Url path: String, @Body body: RequestBody,
        @HeaderMap headers: Map<String, String> = emptyMap()): Response<JsonElement>
    @Streaming @GET suspend fun download(@Url path: String): Response<ResponseBody>
}

fun apiAt(url: String, client: OkHttpClient): MaintApi = Retrofit.Builder().baseUrl(url)
    .client(client).addConverterFactory(wire.asConverterFactory("application/json".toMediaType()))
    .build().create(MaintApi::class.java)
fun JsonObject.requestBody() = toString().toRequestBody("application/json".toMediaType())
fun <T> Response<T>.checked(): T? {
    if (isSuccessful) return body()
    val error = errorBody()?.use { body ->
        runCatching { wire.parseToJsonElement(body.string()).jsonObject }.getOrNull()
    }
    throw ApiFailure(code(), error?.text("message") ?: when (code()) {
        401 -> "Session expirée : reconnectez-vous"
        403 -> "Droits insuffisants"
        409 -> "État incompatible avec cette action"
        412 -> "L’intervention a été modifiée sur le serveur"
        422 -> "Opération refusée par le serveur"
        else -> "Erreur HTTP ${code()}"
    }, error?.text("code"))
}
fun JsonElement?.objectOrEmpty() = this as? JsonObject ?: JsonObject(emptyMap())
fun JsonElement?.objects(): List<JsonObject> = when (this) {
    is JsonArray -> mapNotNull { it as? JsonObject }
    is JsonObject -> list("items")
    else -> emptyList()
}
