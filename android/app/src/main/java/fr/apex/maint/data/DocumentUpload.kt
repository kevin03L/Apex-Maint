package fr.apex.maint.data

import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.MultipartBody
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.asRequestBody
import okio.BufferedSink
import java.io.File
import java.io.IOException

class DocumentFileFailure(cause: Throwable) : IOException(
    "Le fichier local est indisponible ou illisible. Sélectionnez-le ou prenez la photo à nouveau.", cause)

internal inline fun <T> documentFile(work: () -> T): T = try { work() }
    catch (e: DocumentFileFailure) { throw e }
    catch (e: IOException) { throw DocumentFileFailure(e) }
    catch (e: SecurityException) { throw DocumentFileFailure(e) }

/** Le fichier privé reste possédé par l'appelant jusqu'à la fin de l'appel suspendu. */
object DocumentUpload {
    fun body(file: File, interventionId: String, name: String, mime: String, photo: Boolean): MultipartBody {
        val source = file.asRequestBody(mime.toMediaTypeOrNull() ?: "application/octet-stream".toMediaType())
        val local = object : RequestBody() {
            override fun contentType() = source.contentType()
            override fun contentLength(): Long = documentFile {
                if (!file.isFile || !file.canRead()) throw java.io.FileNotFoundException()
                source.contentLength()
            }
            override fun writeTo(sink: BufferedSink) {
                // Seules les lectures locales sont requalifiées : une écriture socket reste une erreur réseau.
                val input = documentFile { file.inputStream() }
                input.use {
                    val buffer = ByteArray(8192)
                    while (true) {
                        val count = documentFile { it.read(buffer) }
                        if (count < 0) break
                        sink.write(buffer, 0, count)
                    }
                }
            }
        }
        return MultipartBody.Builder().setType(MultipartBody.FORM)
            .addFormDataPart("entity_type", "intervention").addFormDataPart("entity_id", interventionId)
            .addFormDataPart("title", name).addFormDataPart("category", if (photo) "photo" else "autre")
            .addFormDataPart("file", name, local).build()
    }
}
