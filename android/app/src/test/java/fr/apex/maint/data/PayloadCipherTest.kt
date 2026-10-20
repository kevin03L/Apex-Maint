package fr.apex.maint.data

import org.junit.Assert.*
import org.junit.Test
import javax.crypto.AEADBadTagException
import javax.crypto.spec.SecretKeySpec

class PayloadCipherTest {
    private val cipher = PayloadCipher { SecretKeySpec(ByteArray(32) { it.toByte() }, "AES") }

    @Test fun `les contenus metier ne sont pas lisibles et se relisent sans perte`() {
        val original = "Compte rendu sensible : étanchéité réparée\nPièce changée"
        val stored = cipher.encrypt("u:s:i", original)
        assertTrue(stored.startsWith("gcm1:"))
        assertFalse(stored.contains("étanchéité"))
        assertEquals(original, cipher.decrypt("u:s:i", stored))
        assertNotEquals(stored, cipher.encrypt("u:s:i", original))
    }

    @Test(expected = AEADBadTagException::class) fun `un autre utilisateur ne peut pas relire le contenu`() {
        cipher.decrypt("autre:s:i", cipher.encrypt("u:s:i", "Rapport"))
    }

    @Test(expected = AEADBadTagException::class) fun `un autre site ne peut pas relire le contenu`() {
        cipher.decrypt("u:autre:i", cipher.encrypt("u:s:i", "Rapport"))
    }

    @Test(expected = AEADBadTagException::class) fun `une autre cle refuse le contenu sans le supprimer`() {
        val stored = cipher.encrypt("u:s:i", "Rapport")
        PayloadCipher { SecretKeySpec(ByteArray(32) { 42 }, "AES") }.decrypt("u:s:i", stored)
    }

    @Test(expected = IllegalArgumentException::class) fun `un contenu tronque est refuse`() {
        cipher.decrypt("u:s:i", "gcm1:AA==")
    }

    @Test(expected = IllegalArgumentException::class) fun `une lecture en clair ne peut pas passer inapercue`() {
        cipher.decrypt("u:s:i", "Rapport en clair")
    }
}
