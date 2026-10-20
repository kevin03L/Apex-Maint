package fr.apex.maint.ui

import org.junit.Assert.*
import org.junit.Test
import java.time.ZoneId

class PresentationTest {
    @Test fun `les dates sont locales et francaises`() {
        assertEquals("20 oct. 2026 à 14:05", frenchDate("2026-10-20T12:05:00.000Z", ZoneId.of("Europe/Paris")))
        assertEquals("20 oct. 2026 à 14:05", frenchDate("2026-10-20T14:05:00+02:00", ZoneId.of("Europe/Paris")))
    }
    @Test fun `une date absente ou invalide reste lisible`() {
        assertEquals("Non renseignée", frenchDate(null))
        assertEquals("Non renseignée", frenchDate("incorrecte"))
    }
    @Test fun `les durees couvrent minutes heures et valeurs invalides`() {
        assertEquals("0 min", frenchDuration(0))
        assertEquals("59 min", frenchDuration(59))
        assertEquals("1 h 00", frenchDuration(60))
        assertEquals("2 h 05", frenchDuration(125))
        assertEquals("0 min", frenchDuration(-1))
    }
    @Test fun `le nombre d operations respecte le singulier et le pluriel`() {
        assertEquals("0 opérations en attente", operationCount(0))
        assertEquals("1 opération en attente", operationCount(1))
        assertEquals("2 opérations en attente", operationCount(2))
    }
}
