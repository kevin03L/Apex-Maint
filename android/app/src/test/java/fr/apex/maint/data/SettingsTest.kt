package fr.apex.maint.data

import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

class SettingsTest {
    @get:Rule val temporary = TemporaryFolder()

    @Test fun `valeur absente ou inconnue choisit sombre`() {
        assertEquals(ThemeChoice.DARK, ThemeChoice.fromStored(null))
        assertEquals(ThemeChoice.DARK, ThemeChoice.fromStored("inconnu"))
        ThemeChoice.entries.forEach { assertEquals(it, ThemeChoice.fromStored(it.stored)) }
    }

    @Test fun `seul systeme suit le mode de appareil`() {
        assertFalse(ThemeChoice.SYSTEM.isDark(false))
        assertTrue(ThemeChoice.SYSTEM.isDark(true))
        listOf(false, true).forEach {
            assertFalse(ThemeChoice.LIGHT.isDark(it))
            assertTrue(ThemeChoice.DARK.isDark(it))
        }
    }

    @Test fun `datastore publie les trois choix sans effacer le serveur`() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        try {
            val store = PreferenceDataStoreFactory.create(scope = scope,
                produceFile = { File(temporary.root, "choix.preferences_pb") })
            val settings = Settings(store)
            val url = stringPreferencesKey("url")
            store.edit { it[url] = "https://atelier.test:8443/" }
            assertEquals(ThemeChoice.DARK, settings.theme.first())
            ThemeChoice.entries.forEach {
                settings.saveTheme(it)
                assertEquals(it, settings.theme.first())
                assertEquals("https://atelier.test:8443/", settings.url())
            }
        } finally { scope.coroutineContext[Job]!!.cancelAndJoin() }
    }

    @Test fun `choix conserve apres recreation du datastore`() = runBlocking {
        val file = File(temporary.root, "persistant.preferences_pb")
        ThemeChoice.entries.forEach { choice ->
            val writer = CoroutineScope(SupervisorJob() + Dispatchers.IO)
            try {
                Settings(PreferenceDataStoreFactory.create(scope = writer, produceFile = { file })).saveTheme(choice)
            } finally { writer.coroutineContext[Job]!!.cancelAndJoin() }
            val reader = CoroutineScope(SupervisorJob() + Dispatchers.IO)
            try {
                assertEquals(choice, Settings(PreferenceDataStoreFactory.create(scope = reader,
                    produceFile = { file })).theme.first())
            } finally { reader.coroutineContext[Job]!!.cancelAndJoin() }
        }
    }
}
