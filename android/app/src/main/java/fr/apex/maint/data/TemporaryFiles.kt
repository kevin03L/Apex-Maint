package fr.apex.maint.data

import java.io.File
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.UUID

/** Copies privées en clair, jamais sauvegardées. Horloge injectable pour la purge. */
class TemporaryFiles(private val cache: File, private val now: () -> Long = System::currentTimeMillis) {
    private val shared = File(cache, "external")
    private val work = File(cache, "document_work")
    private var departed = shared.listFiles()?.toList().orEmpty()
    fun create(extension: String = "bin", external: Boolean = true): File {
        val folder = (if (external) shared else work).apply { check(mkdirs() || isDirectory) }
        val suffix = extension.filter { it.isLetterOrDigit() }.take(10).ifBlank { "bin" }
        return File(folder, "${UUID.randomUUID()}.$suffix").apply { check(createNewFile()); setLastModified(now()) }
    }
    fun purgeExpired(maxAgeMillis: Long = 5 * 60_000L) {
        listOf(shared, work, File(cache, "documents"), File(cache, "photos")).forEach { folder ->
            folder.listFiles()?.filter { it.isFile && now() - it.lastModified() >= maxAgeMillis }
                ?.forEach { it.delete() }
        }
    }
    fun claimPhoto(name: String): File {
        require(name == File(name).name) { "Nom de capture invalide" }
        val source = File(shared, name)
        val target = create("jpg", external = false)
        try { Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING); return target }
        catch (e: Exception) { target.delete(); source.delete(); throw e }
    }
    fun paused() { departed = shared.listFiles()?.toList().orEmpty() }
    // Une capture créée par le retour de permission appartient au prochain départ, pas au précédent.
    // Les captures réussies sont déjà déplacées vers le travail privé avant cette reprise.
    fun resumed() { departed.filter { it.isFile }.forEach { it.delete() }; departed = emptyList() }
    fun clear() {
        listOf(shared, work, File(cache, "documents"), File(cache, "photos")).forEach { folder ->
            folder.listFiles()?.filter { it.isFile }?.forEach { it.delete() }
        }
        departed = emptyList()
    }
}
