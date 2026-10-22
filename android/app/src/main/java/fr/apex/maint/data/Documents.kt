package fr.apex.maint.data

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import androidx.core.content.FileProvider
import java.io.File

class Documents(private val context: Context, private val api: () -> MaintApi) {
    private val temporary = TemporaryFiles(context.cacheDir)
    fun claimPhoto(uri: Uri): Uri = Uri.fromFile(temporary.claimPhoto(requireNotNull(uri.lastPathSegment)))
    fun deletePhoto(uri: Uri) { context.contentResolver.delete(uri, null, null) }
    // Les deux plafonds serveur ont un minimum de 1 Mio, mais seul le plafond documentaire
    // est exposé aux comptes settings.view. Ce client utilise le minimum sûr pour tous.
    val limit = 1024L * 1024
    suspend fun upload(actor: Actor, interventionId: String, uri: Uri, photo: Boolean) {
        require(actor.can("documents.create")) { "Ajout de documents non autorisé" }
        require(!interventionId.startsWith("temp_")) { "Synchronisez d’abord l’intervention" }
        val resolver = context.contentResolver
        val name = if (photo) "photo.jpg" else documentFile { resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        } } ?: "document"
        val safeName = if (photo) "photo.jpg" else name.substringAfterLast('/').substringAfterLast('\\')
            .filter { it >= ' ' && it != '"' }.take(180).ifBlank { "document" }
        val file = documentFile { temporary.create(external = false) }
        try {
            documentFile {
                (resolver.openInputStream(uri) ?: throw java.io.FileNotFoundException()).use { input -> file.outputStream().use { output ->
                    val buffer = ByteArray(8192); var size = 0L
                    while (true) {
                        val count = input.read(buffer); if (count < 0) break
                        size += count
                        require(size <= if (photo) 20 * limit else limit) { "Le fichier dépasse la limite mobile" }
                        output.write(buffer, 0, count)
                    }
                } }
                if (photo) File(requireNotNull(uri.path)).delete()
                if (photo) {
                    val orientation = runCatching { ExifInterface(file.path).getAttributeInt(
                        ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL) }.getOrDefault(ExifInterface.ORIENTATION_NORMAL)
                    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeFile(file.path, bounds)
                    var sample = 1
                    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > 1600) sample *= 2
                    val bitmap = requireNotNull(BitmapFactory.decodeFile(file.path,
                        BitmapFactory.Options().apply { inSampleSize = sample })) { "Photo illisible" }
                    val rotation = when (orientation) {
                        ExifInterface.ORIENTATION_ROTATE_90, ExifInterface.ORIENTATION_TRANSPOSE -> 90f
                        ExifInterface.ORIENTATION_ROTATE_180, ExifInterface.ORIENTATION_FLIP_VERTICAL -> 180f
                        ExifInterface.ORIENTATION_ROTATE_270, ExifInterface.ORIENTATION_TRANSVERSE -> 270f
                        else -> 0f
                    }
                    val matrix = Matrix().apply {
                        postRotate(rotation)
                        if (orientation in setOf(ExifInterface.ORIENTATION_FLIP_HORIZONTAL, ExifInterface.ORIENTATION_FLIP_VERTICAL,
                                ExifInterface.ORIENTATION_TRANSPOSE, ExifInterface.ORIENTATION_TRANSVERSE)) postScale(-1f, 1f)
                    }
                    val upright = Bitmap.createBitmap(bitmap, 0, 0, bitmap.width, bitmap.height, matrix, true)
                    try { file.outputStream().use { upright.compress(Bitmap.CompressFormat.JPEG, 80, it) } }
                    finally { if (upright !== bitmap) upright.recycle(); bitmap.recycle() }
                }
                require(file.length() <= limit) { "Le document dépasse la limite mobile de 1 Mio" }
            }
            val mime = if (photo) "image/jpeg" else documentFile { resolver.getType(uri) } ?: "application/octet-stream"
            val multipart = DocumentUpload.body(file, interventionId, safeName, mime, photo)
            api().post("api/documents", multipart).checked()
        } catch (e: Exception) {
            debugFailure("envoi documentaire", e)
            throw e
        } finally {
            file.delete()
            if (photo) File(requireNotNull(uri.path)).delete()
        }
    }
    suspend fun download(actor: Actor, documentId: String, extension: String): File {
        require(actor.can("documents.view")) { "Lecture non autorisée" }
        val suffix = extension.filter { it.isLetterOrDigit() }.take(10).ifBlank { "bin" }
        val file = temporary.create(suffix)
        try {
            requireNotNull(api().download("api/documents/${encode(documentId)}/download?download=1").checked()).use { body ->
                body.byteStream().use { input -> file.outputStream().use { output -> input.copyTo(output) } }
            }
            return file
        } catch (e: Exception) { file.delete(); throw e }
    }
    fun open(file: File, mime: String) {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        try { context.startActivity(intent) }
        catch (e: Exception) { file.delete(); throw e }
    }
}
