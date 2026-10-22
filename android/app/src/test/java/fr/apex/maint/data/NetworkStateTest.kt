package fr.apex.maint.data

import org.junit.Assert.*
import org.junit.Test

class NetworkStateTest {
    private var online = true
    private val state = NetworkState<String> { online = it }
    @Test fun `demarrage et reprise sans reseau effacent un ancien etat en ligne`() {
        state.refresh(null); assertFalse(online)
        state.refresh("wifi"); assertTrue(online)
        state.refresh(null); assertFalse(online)
    }
    @Test fun `reseau local sans validation internet suffit des sa disponibilite`() {
        state.refresh(null)
        state.available("atelier"); assertTrue(online)
        state.refresh("atelier"); assertTrue(online)
    }
    @Test fun `perte du defaut est publiee directement`() {
        state.refresh("wifi")
        state.lost("wifi"); assertFalse(online)
    }
    @Test fun `bascule ignore la perte de lancien reseau`() {
        state.refresh("wifi")
        state.available("mobile"); assertTrue(online)
        state.lost("wifi"); assertTrue(online)
        state.lost("mobile"); assertFalse(online)
    }
    @Test fun `reprise retrouve le reseau sans attendre de nouveau callback`() {
        state.refresh("wifi"); state.lost("wifi")
        state.refresh("wifi"); assertTrue(online)
    }
}
