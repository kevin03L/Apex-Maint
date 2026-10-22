package fr.apex.maint.ui

import android.content.res.Configuration
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import fr.apex.maint.data.*
import fr.apex.maint.ui.theme.*

@Preview(name = "Téléphone clair", widthDp = 360, heightDp = 800, uiMode = Configuration.UI_MODE_NIGHT_NO)
@Preview(name = "Téléphone sombre", widthDp = 360, heightDp = 800, uiMode = Configuration.UI_MODE_NIGHT_YES)
@Preview(name = "Tablette claire", widthDp = 1280, heightDp = 800, uiMode = Configuration.UI_MODE_NIGHT_NO)
@Preview(name = "Tablette sombre", widthDp = 1280, heightDp = 800, uiMode = Configuration.UI_MODE_NIGHT_YES)
@Preview(name = "Téléphone police 200 %", widthDp = 360, heightDp = 800, fontScale = 2f,
    uiMode = Configuration.UI_MODE_NIGHT_YES)
private annotation class ApexDevices

// Aperçus sans conteneur réseau/Keystore : mêmes composants que les écrans réels.
@ApexDevices
@Composable private fun ConnexionPreview() {
    ApexTheme(isSystemInDarkTheme()) {
        Page {
            Text("Connexion", style = MaterialTheme.typography.headlineSmall)
            Text("https://maint.atelier.local:8443/", color = LocalApexTokens.current.muted)
            var user by remember { mutableStateOf("atelier") }
            Field("Identifiant", user, { user = it })
            Field("Mot de passe", "", {}, secret = true)
            Action("Se connecter") {}
            ThemeOptions(ThemeChoice.SYSTEM) {}
        }
    }
}

private val demoIntervention = obj("id" to "exemple".j(), "code" to "INT-2026-0042".j(),
    "title" to "Remplacement du roulement du convoyeur de la ligne de conditionnement".j(),
    "status" to "in_progress".j(), "priority" to "high".j())

@ApexDevices
@Composable private fun TerrainPreview() {
    ApexTheme(isSystemInDarkTheme()) {
        EntityListPage(Modifier, rememberLazyListState(), false, {}, listOf(demoIntervention),
            header = {
                Text("Interventions", style = MaterialTheme.typography.headlineSmall)
                Action("Déclarer une intervention") {}
            }, empty = {}) { item -> EntityCard(item, item.text("title").orEmpty(), "", selected = true) {} }
    }
}

@ApexDevices
@Composable private fun DetailEtDeclarationPreview() {
    ApexTheme(isSystemInDarkTheme()) {
        Page {
            Text("Intervention", style = MaterialTheme.typography.headlineSmall)
            EntityCard(demoIntervention, demoIntervention.text("title").orEmpty(), "") {}
            Section("Déclaration", Icons.Default.Assignment) {
                Field("Titre obligatoire", demoIntervention.text("title").orEmpty(), {}, limit = 128)
                Field("Description", "Vérifier le convoyeur avant la remise en service.", {}, lines = 3)
            }
            Section("Documents de l’équipement", Icons.Default.Description) {
                ApexOutlinedButton({}) { Text("Procédure de maintenance du convoyeur et contrôle après intervention.pdf") }
            }
            Action("Enregistrer la description et le compte rendu") {}
        }
    }
}
