package fr.apex.maint.data

import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Le contexte authentifié empêche de déplacer un contenu vers une autre ligne. */
class PayloadCipher(private val key: () -> SecretKey) {
    fun encrypt(context: String, value: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        cipher.updateAAD(context.toByteArray(Charsets.UTF_8))
        return "gcm1:" + Base64.getEncoder().encodeToString(cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8)))
    }

    fun decrypt(context: String, value: String): String {
        require(value.startsWith("gcm1:")) { "Contenu local non chiffré" }
        val bytes = Base64.getDecoder().decode(value.removePrefix("gcm1:"))
        require(bytes.size >= 28) { "Contenu local incomplet" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        cipher.updateAAD(context.toByteArray(Charsets.UTF_8))
        return cipher.doFinal(bytes.copyOfRange(12, bytes.size)).toString(Charsets.UTF_8)
    }
}
