package fr.apex.maint.data

import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

class TemporaryFilesTest {
    @get:Rule val directory = TemporaryFolder()
    @Test fun `purge a cinq minutes avec horloge injectable et anciens dossiers`() {
        var now = 1_000_000L
        val cache = directory.newFolder()
        val files = TemporaryFiles(cache) { now }
        val old = files.create("pdf")
        val legacy = File(File(cache, "photos").apply { mkdirs() }, "ancienne.jpg").apply {
            writeText("photo"); setLastModified(now)
        }
        now += 299_999
        files.purgeExpired()
        assertTrue(old.exists()); assertTrue(legacy.exists())
        val recent = files.create()
        now++
        files.purgeExpired()
        assertFalse(old.exists()); assertFalse(legacy.exists()); assertTrue(recent.exists())
    }
    @Test fun `reprise supprime les copies externes et conserve lenvoi en cours`() {
        val files = TemporaryFiles(directory.newFolder())
        val document = files.create("pdf")
        val capture = files.create("jpg").apply { writeText("capture") }
        files.paused()
        val claimed = files.claimPhoto(capture.name)
        assertFalse(capture.exists()); assertEquals("capture", claimed.readText())
        files.resumed()
        assertFalse(document.exists()); assertTrue(claimed.exists())
        files.clear()
        assertFalse(claimed.exists())
        // Retour de permission : la photo vient d’être créée pour le prochain départ.
        files.paused()
        val nextCapture = files.create("jpg")
        files.resumed()
        assertTrue(nextCapture.exists())
        files.paused(); files.resumed()
        assertFalse(nextCapture.exists())
    }
    @Test fun `purge reste limitee aux fichiers temporaires de ce client`() {
        val cache = directory.newFolder()
        val untouched = File(cache, "autre").apply { writeText("conserver") }
        val files = TemporaryFiles(cache)
        assertNotEquals(files.create().name, files.create().name)
        files.clear()
        assertTrue(untouched.exists())
        try { files.claimPhoto("../autre"); fail() } catch (_: IllegalArgumentException) { }
        assertTrue(untouched.exists())
    }
}
