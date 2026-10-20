package fr.apex.maint.ui

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.compose.*
import fr.apex.maint.data.*
import kotlinx.serialization.json.*
import java.io.File
import java.time.Instant
import java.util.UUID

private val labels = mapOf(
    "open" to "Ouverte", "in_progress" to "En cours", "completed" to "Clôturée", "cancelled" to "Annulée",
    "low" to "Basse", "medium" to "Normale", "high" to "Haute", "critical" to "Critique",
    "corrective" to "Corrective", "preventive" to "Préventive", "hors_equipement" to "Hors équipement",
    "a_posteriori" to "A posteriori", "running" to "En marche", "stopped" to "À l’arrêt",
    "FONCTIONNEL" to "Fonctionnelle", "ARRET" to "À l’arrêt", "DEGRADE" to "Dégradée",
    "operational" to "Opérationnel", "out_of_service" to "Hors service",
    "degraded" to "Dégradée", "unknown" to "Inconnu", "pending" to "En attente", "conflict" to "À résoudre",
    "take_intervention" to "Prise en charge", "record_time" to "Saisie du temps",
    "complete_intervention" to "Clôture", "patch_intervention" to "Modification",
    "create_intervention" to "Création",
)
private fun label(value: String?) = labels[value] ?: value.orEmpty()

@Composable private fun Page(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp), content = content)
}
@Composable private fun Field(label: String, value: String, change: (String) -> Unit,
    secret: Boolean = false, lines: Int = 1) {
    OutlinedTextField(value, change, label = { Text(label) }, modifier = Modifier.fillMaxWidth(),
        minLines = lines, singleLine = lines == 1,
        visualTransformation = if (secret) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None)
}
@Composable private fun Action(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    Button(onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(text) }
}
@Composable private fun Picker(title: String, selected: String, choices: List<Pair<String, String>>, choose: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        OutlinedButton({ expanded = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
            Text("$title : ${choices.find { it.first == selected }?.second ?: selected}")
        }
        DropdownMenu(expanded, { expanded = false }) {
            choices.forEach { (id, name) -> DropdownMenuItem(text = { Text(name) }, onClick = { choose(id); expanded = false }) }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable fun MaintApp(vm: MaintViewModel) {
    val s by vm.state.collectAsStateWithLifecycle()
    val nav = rememberNavController()
    val wide = currentWindowAdaptiveInfo().windowSizeClass.isWidthAtLeastBreakpoint(600)
    var logout by remember { mutableStateOf(false) }
    val snack = remember { SnackbarHostState() }
    LaunchedEffect(s.message) { s.message?.let { snack.showSnackbar(it); vm.message(null) } }
    LaunchedEffect(s.actor?.scope, s.expired) {
        if (s.actor != null && !s.expired) nav.navigate("terrain") { popUpTo(nav.graph.id) { inclusive = true } }
    }
    Scaffold(snackbarHost = { SnackbarHost(snack) }, topBar = {
        TopAppBar(title = { Text("Apex Maint") }, actions = {
            if (s.session != null) IconButton({ logout = true }) { Icon(Icons.Default.Logout, "Se déconnecter") }
        })
    }) { inset ->
        Column(Modifier.padding(inset).fillMaxSize()) {
            if (s.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            if (!s.ready) { Text("Chargement…", Modifier.padding(24.dp)); return@Column }
            when {
                s.server == null -> ServerScreen(vm, s)
                s.session == null || s.expired -> LoginScreen(vm, s)
                s.session!!.must_change_password || s.session!!.user.must_change_password -> PasswordScreen(vm, s)
                s.actor == null -> Page {
                    Text("Choisissez votre site", style = MaterialTheme.typography.headlineSmall)
                    LaunchedEffect(Unit) { vm.sites() }
                    s.sites.forEach { site -> Action(site.text("name").orEmpty(), !s.busy) { vm.selectSite(requireNotNull(site.text("id"))) } }
                    Text("Le site d’un compte non administrateur est fixé par le serveur.")
                }
                else -> {
                    Surface(color = MaterialTheme.colorScheme.primaryContainer) {
                        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                            Text(if (s.online) "${s.operations.size} opération(s) en attente" else
                                "Hors ligne · ${s.operations.size} opération(s)", Modifier.weight(1f).padding(vertical = 12.dp))
                            IconButton({ vm.sync() }, enabled = !s.busy && s.online) { Icon(Icons.Default.Sync, "Synchroniser") }
                        }
                    }
                    val entries = buildList {
                        if (s.actor!!.can("interventions.view")) add(Triple("terrain", "Terrain", Icons.Default.Build))
                        if (s.actor!!.can("parts.view")) add(Triple("stock", "Pièces", Icons.Default.Inventory2))
                        if (s.actor!!.can("equipments.view")) add(Triple("scanner", "Scanner", Icons.Default.QrCodeScanner))
                        add(Triple("sync", "Synchronisation", Icons.Default.Sync))
                    }
                    val route = nav.currentBackStackEntryAsState().value?.destination?.route
                    val currentId = nav.currentBackStackEntryAsState().value?.arguments?.getString("id")
                    LaunchedEffect(currentId, s.aliases) {
                        s.aliases[currentId]?.let { real -> nav.navigate("terrain/$real") {
                            popUpTo("terrain/{id}") { inclusive = true }
                        } }
                    }
                    Row(Modifier.weight(1f)) {
                        if (wide) NavigationRail {
                            entries.forEach { (id, title, icon) -> NavigationRailItem(
                                selected = route?.startsWith(id) == true,
                                onClick = { nav.navigate(id) { launchSingleTop = true } },
                                icon = { Icon(icon, title) }, label = { Text(title) }) }
                        }
                        NavHost(nav, startDestination = "terrain", modifier = Modifier.weight(1f)) {
                            composable("terrain") { Terrain(vm, s, wide, null, { nav.navigate("terrain/$it") }, { nav.navigate("creation") }) }
                            composable("terrain/{id}") { entry -> Terrain(vm, s, wide, entry.arguments?.getString("id"),
                                { nav.navigate("terrain/$it") }, { nav.navigate("creation") }) }
                            composable("creation") { CreateScreen(vm, s, { nav.navigate("scanner") }) { nav.navigate("terrain/$it") } }
                            composable("stock") { StockScreen(vm, s, wide, null) { nav.navigate("stock/$it") } }
                            composable("stock/{id}") { entry -> StockScreen(vm, s, wide, entry.arguments?.getString("id")) { nav.navigate("stock/$it") } }
                            composable("scanner") {
                                if (s.actor!!.can("equipments.view")) ScannerScreen { value -> vm.scan(value) { collection, id ->
                                    if (collection == "equipments" && nav.previousBackStackEntry?.destination?.route == "creation") {
                                        vm.equipmentForForm(id); nav.popBackStack()
                                    } else nav.navigate(if (collection == "equipments") "equipement/$id" else "terrain/$id")
                                } }
                            }
                            composable("equipement/{id}") { entry -> EquipmentScreen(vm, s,
                                entry.arguments?.getString("id").orEmpty(), { nav.navigate("creation") }, { nav.navigate("terrain/$it") }) }
                            composable("sync") { SyncScreen(vm, s) }
                        }
                    }
                    if (!wide) NavigationBar {
                        entries.forEach { (id, title, icon) -> NavigationBarItem(
                            selected = route?.startsWith(id) == true,
                            onClick = { nav.navigate(id) { launchSingleTop = true } },
                            icon = { Icon(icon, title) }, label = { Text(title) }) }
                    }
                }
            }
        }
    }
    if (logout) AlertDialog(onDismissRequest = { logout = false }, title = { Text("Se déconnecter") },
        text = { Text("Le cache sera effacé. ${s.operations.size} opération(s) non synchronisée(s) seront perdues.") },
        confirmButton = { TextButton({ logout = false; vm.logout() }) { Text("Se déconnecter") } },
        dismissButton = { TextButton({ logout = false }) { Text("Annuler") } })
}

@Composable private fun ServerScreen(vm: MaintViewModel, s: UiState) {
    var url by rememberSaveable { mutableStateOf("https://") }
    Page {
        Text("Configurer le serveur", style = MaterialTheme.typography.headlineSmall)
        Field("URL de l’instance", url, { url = it })
        Text("Exemple : https://maint.atelier.local:8443/")
        Text("Autorité privée : installez le certificat CA de l’instance dans les paramètres de sécurité Android. Le nom du serveur doit figurer dans le certificat.")
        Action("Enregistrer", !s.busy) { vm.server(url) }
    }
}
@Composable private fun LoginScreen(vm: MaintViewModel, s: UiState) {
    var user by rememberSaveable { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val context = LocalContext.current
    Page {
        Text("Connexion", style = MaterialTheme.typography.headlineSmall)
        Text(s.server.orEmpty())
        if (s.expired) Text("Votre session a expiré. Reconnectez-vous avec le même compte pour conserver la file.")
        Field("Identifiant", user, { user = it })
        Field("Mot de passe", password, { password = it }, secret = true)
        Action("Se connecter", !s.busy && user.isNotBlank() && password.isNotEmpty()) {
            vm.login(user, password); password = ""
        }
        OutlinedButton({ context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(s.server + "api/tls/ca.pem"))) }) {
            Text("Télécharger l’autorité dans le navigateur")
        }
        Text("Après vérification de son empreinte, installez cette autorité comme certificat CA. Un premier transfert peut nécessiter votre administrateur.")
        if (s.session == null) {
            var edit by remember { mutableStateOf(false) }
            TextButton({ edit = !edit }) { Text("Modifier l’URL") }
            if (edit) { var url by rememberSaveable { mutableStateOf(s.server.orEmpty()) }
                Field("URL du serveur", url, { url = it }); Action("Enregistrer", !s.busy) { vm.server(url) } }
        }
    }
}
@Composable private fun PasswordScreen(vm: MaintViewModel, s: UiState) {
    var current by remember { mutableStateOf("") }; var new by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    Page {
        Text("Changement de mot de passe obligatoire", style = MaterialTheme.typography.headlineSmall)
        Field("Mot de passe actuel", current, { current = it }, true)
        Field("Nouveau mot de passe", new, { new = it }, true)
        Field("Confirmation", confirm, { confirm = it }, true)
        Action("Changer le mot de passe", !s.busy && current.isNotEmpty() && new.isNotEmpty() && new == confirm) {
            vm.password(current, new); current = ""; new = ""; confirm = ""
        }
    }
}

@Composable private fun Terrain(vm: MaintViewModel, s: UiState, wide: Boolean, id: String?,
    select: (String) -> Unit, create: () -> Unit) {
    if (s.actor?.can("interventions.view") != true) { Page { Text("Consultation des interventions non autorisée") }; return }
    LaunchedEffect(id) { if (id != null) vm.select("interventions", id) }
    Row(Modifier.fillMaxSize()) {
        if (wide || id == null) Page(Modifier.weight(1f)) {
            Text("Interventions", style = MaterialTheme.typography.headlineSmall)
            var mine by rememberSaveable { mutableStateOf(true) }
            var status by rememberSaveable { mutableStateOf("") }
            Row { Checkbox(mine, { mine = it; if (!it && s.online) vm.siteList() }); Text("Mes interventions", Modifier.padding(top = 12.dp)) }
            Picker("Statut", status, listOf("" to "Tous") + listOf("open", "in_progress", "completed", "cancelled").map { it to label(it) }) { status = it }
            if (!mine && s.online) Action("Recharger les interventions du site", !s.busy) { vm.siteList() }
            if (s.actor?.can("interventions.create") == true) Action("Déclarer une intervention", !s.busy, create)
            val items = s.interventions.filter { item -> (status.isEmpty() || item.text("status") == status) &&
                (!mine || item.list("technicians").any { it.text("user_id") == s.actor?.userId }) }
            if (items.isEmpty()) Text("Aucune intervention dans ce périmètre.")
            items.forEach { item -> EntityCard(item, item.text("title").orEmpty(),
                "${label(item.text("status"))} · ${label(item.text("priority"))}") { select(requireNotNull(item.text("id"))) } }
        }
        if (wide || id != null) {
            if (wide) VerticalDivider()
            if (id == null) Page(Modifier.weight(1f)) { Text("Sélectionnez une intervention") }
            else InterventionScreen(vm, s, id, Modifier.weight(1f), select)
        }
    }
}
@Composable private fun EntityCard(item: JsonObject, title: String, subtitle: String, click: () -> Unit) {
    Card(Modifier.fillMaxWidth().clickable(onClick = click).heightIn(min = 64.dp)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(item.text("code") ?: item.text("reference").orEmpty(), style = MaterialTheme.typography.labelLarge)
            Text(title, style = MaterialTheme.typography.titleMedium); Text(subtitle)
        }
    }
}
@Composable private fun InterventionScreen(vm: MaintViewModel, s: UiState, id: String, modifier: Modifier, select: (String) -> Unit) {
    val item = s.selected?.takeIf { s.resourceKey == "interventions:$id" }
    Page(modifier) {
        if (item == null) { Text("Chargement de l’intervention…"); return@Page }
        Text(item.text("code").orEmpty(), style = MaterialTheme.typography.labelLarge)
        Text(item.text("title").orEmpty(), style = MaterialTheme.typography.headlineSmall)
        Text("${label(item.text("status"))} · ${label(item.text("priority"))}")
        Text("Temps total : ${item.text("duration_minutes") ?: "0"} min")
        var description by rememberSaveable(id, item.text("description")) { mutableStateOf(item.text("description").orEmpty()) }
        var report by rememberSaveable(id, item.text("solution_report")) { mutableStateOf(item.text("solution_report").orEmpty()) }
        var time by rememberSaveable(id) { mutableStateOf(item.list("technicians").find { it.text("user_id") == s.actor?.userId }?.text("duration_minutes") ?: "0") }
        val editable = s.actor?.can("interventions.edit") == true && item.text("status") in setOf("open", "in_progress")
        if (editable) {
            if (item.text("status") == "open") Action("Prendre en charge", !s.busy) { vm.mutate(id, "take_intervention", obj()) }
            Field("Description", description, { description = it }, lines = 3)
            Field("Compte rendu", report, { report = it }, lines = 3)
            Action("Enregistrer les modifications", !s.busy) { vm.mutate(id, "patch_intervention",
                obj("description" to description.j(), "solution_report" to report.j())) }
            Field("Mon temps total en minutes", time, { time = it })
            Action("Enregistrer mon temps", !s.busy && (time.toIntOrNull() ?: -1) >= 0) {
                vm.mutate(id, "record_time", obj("duration_minutes" to requireNotNull(time.toIntOrNull()).j())) }
            var complete by remember { mutableStateOf(false) }
            Action("Clôturer avec le compte rendu", !s.busy && report.isNotBlank()) { complete = true }
            if (complete) AlertDialog(onDismissRequest = { complete = false }, title = { Text("Clôturer l’intervention") },
                text = { Text(report) }, confirmButton = { TextButton({ complete = false
                    vm.mutate(id, "complete_intervention", obj("solution_report" to report.j())) }) { Text("Clôturer") } },
                dismissButton = { TextButton({ complete = false }) { Text("Annuler") } })
        } else { Text(item.text("description").orEmpty()); Text(item.text("solution_report").orEmpty()) }
        Text("Sous-interventions", style = MaterialTheme.typography.titleMedium)
        s.children.forEach { child -> EntityCard(child, child.text("title").orEmpty(), label(child.text("status"))) { select(requireNotNull(child.text("id"))) } }
        Text("Pièces consommées", style = MaterialTheme.typography.titleMedium)
        s.consumed.forEach { Text("${it.text("part_reference")} · ${it.text("part_name")} : ${it.text("quantity")} (${it.text("location_name")})") }
        if (!s.online) Text("Pièces et documents : disponibles en ligne.")
        if (s.online && !id.startsWith("temp_") && s.actor?.can("stock.movements") == true)
            ConsumeForm(vm, s, id)
        DocumentSection(vm, s, id)
    }
}

@Composable private fun CreateScreen(vm: MaintViewModel, s: UiState, scan: () -> Unit, done: (String) -> Unit) {
    var title by rememberSaveable { mutableStateOf("") }; var description by rememberSaveable { mutableStateOf("") }
    var search by rememberSaveable { mutableStateOf("") }; var equipment by rememberSaveable(s.formEquipment) { mutableStateOf(s.formEquipment) }
    var priority by rememberSaveable { mutableStateOf("medium") }; var machine by rememberSaveable { mutableStateOf("FONCTIONNEL") }
    var type by rememberSaveable { mutableStateOf("corrective") }; var report by rememberSaveable { mutableStateOf("") }
    var minutes by rememberSaveable { mutableStateOf("0") }
    Page {
        Text("Déclarer une intervention", style = MaterialTheme.typography.headlineSmall)
        Field("Titre", title, { title = it }); Field("Description", description, { description = it }, lines = 3)
        Picker("Type", type, listOf("corrective", "preventive", "hors_equipement", "a_posteriori").map { it to label(it) }) { type = it }
        Picker("Priorité", priority, listOf("low", "medium", "high", "critical").map { it to label(it) }) { priority = it }
        Picker("État machine", machine, listOf("FONCTIONNEL", "ARRET", "DEGRADE").map { it to label(it) }) { machine = it }
        Action("Scanner un équipement", onClick = scan)
        Field("Rechercher un équipement", search, { search = it })
        Text("Équipement : " + (s.equipments.find { it.text("id") == equipment }?.let { "${it.text("code")} · ${it.text("name")}" } ?: "Non sélectionné"))
        if (search.isNotBlank()) s.equipments.filter { "${it.text("code")} ${it.text("name")}".contains(search, true) }.take(30).forEach { e ->
            OutlinedButton({ equipment = e.text("id"); search = "" }, Modifier.fillMaxWidth()) { Text("${e.text("code")} · ${e.text("name")}") }
        }
        if (type == "a_posteriori") { Field("Compte rendu obligatoire", report, { report = it }, lines = 3)
            Field("Durée en minutes", minutes, { minutes = it }) }
        val valid = title.isNotBlank() && (equipment != null || type == "hors_equipement") &&
            (type != "a_posteriori" || (report.isNotBlank() && (minutes.toIntOrNull() ?: -1) >= 0))
        Action("Créer l’intervention", valid && !s.busy && s.actor?.can("interventions.create") == true) {
            val fields = mutableMapOf<String, JsonElement>("title" to title.j(), "description" to description.j(),
                "type" to type.j(), "priority" to priority.j(), "machine_state" to machine.j(),
                "equipment_id" to if (type == "hors_equipement") JsonNull else equipment?.j() ?: JsonNull)
            if (type == "a_posteriori") {
                val duration = requireNotNull(minutes.toIntOrNull()); val now = Instant.now()
                fields.putAll(mapOf("solution_report" to report.j(), "duration_minutes" to duration.j(),
                    "started_at" to now.minusSeconds(duration * 60L).toString().j(), "completed_at" to now.toString().j()))
            }
            vm.create(JsonObject(fields), done)
        }
        if (!s.online) Text("Cette déclaration sera synchronisée au retour du réseau.")
    }
}

@Composable private fun StockScreen(vm: MaintViewModel, s: UiState, wide: Boolean, id: String?, select: (String) -> Unit) {
    if (s.actor?.can("parts.view") != true) { Page { Text("Consultation des pièces non autorisée") }; return }
    LaunchedEffect(id) { if (id != null) vm.select("parts", id) }
    Row {
        if (wide || id == null) Page(Modifier.weight(1f)) {
            Text("Catalogue des pièces", style = MaterialTheme.typography.headlineSmall)
            var q by rememberSaveable { mutableStateOf("") }
            Field("Rechercher une pièce", q, { q = it })
            Action("Actualiser le catalogue", s.online && !s.busy) { vm.catalogue() }
            s.parts.filter { "${it.text("reference")} ${it.text("name")}".contains(q, true) }.forEach { part ->
                EntityCard(part, part.text("name").orEmpty(), "Disponible : ${part.text("total_available") ?: part.text("available") ?: "—"}") {
                    select(requireNotNull(part.text("id"))) }
            }
        }
        if (wide || id != null) Page(Modifier.weight(1f)) {
            val part = s.selected?.takeIf { s.resourceKey == "parts:$id" }
            if (part == null) Text("Sélectionnez une pièce") else {
                Text(part.text("reference").orEmpty()); Text(part.text("name").orEmpty(), style = MaterialTheme.typography.headlineSmall)
                Text(part.text("description").orEmpty())
                Text("Disponible : ${part.text("total_available") ?: part.text("available") ?: "—"}")
                part.list("locations").forEach { location -> Text("${location.text("location_code")} · ${location.text("location_name")} : " +
                    "${location.text("quantity")} en stock, ${location.text("available_quantity")} disponibles, ${location.text("reserved_quantity")} réservés") }
                if (!s.online) Text("Les quantités par emplacement nécessitent un chargement préalable en ligne.")
            }
        }
    }
}

@Composable private fun ConsumeForm(vm: MaintViewModel, s: UiState, interventionId: String) {
    if (s.actor?.can("interventions.edit") != true) return
    var open by remember { mutableStateOf(false) }; var part by remember { mutableStateOf<JsonObject?>(null) }
    var location by remember { mutableStateOf("") }; var quantity by remember { mutableStateOf("1") }
    OutlinedButton({ open = !open; if (open) vm.catalogue() }) { Text("Sortir une pièce") }
    if (open) {
        var search by remember { mutableStateOf("") }
        Field("Rechercher la pièce à sortir", search, { search = it })
        s.parts.filter { "${it.text("reference")} ${it.text("name")}".contains(search, true) }.take(20).forEach { candidate ->
            OutlinedButton({ location = ""; vm.part(requireNotNull(candidate.text("id"))) { part = it } }) { Text("${candidate.text("reference")} · ${candidate.text("name")}") }
        }
        // Les détails de pièce sont chargés séparément pour conserver la fiche intervention visible.
        val locations = part?.list("locations").orEmpty()
        if (part != null) {
            Picker("Emplacement", location, locations.map { requireNotNull(it.text("location_id")) to
                "${it.text("location_name")} (${it.text("available_quantity")} disponibles)" }) { location = it }
            Field("Quantité", quantity, { quantity = it })
            Action("Confirmer la sortie", !s.busy && location.isNotBlank() && (quantity.toIntOrNull() ?: 0) > 0) {
                vm.consume(interventionId, requireNotNull(part?.text("id")), location, requireNotNull(quantity.toIntOrNull())); open = false
            }
        }
    }
}

@Composable private fun ColumnScope.DocumentSection(vm: MaintViewModel, s: UiState, interventionId: String?) {
    if (s.actor?.can("documents.view") == true) {
        Text("Documents", style = MaterialTheme.typography.titleMedium)
        s.documents.forEach { doc -> OutlinedButton({ vm.openDocument(doc) }, enabled = s.online && !s.busy) { Text(doc.text("title").orEmpty()) } }
    }
    if (interventionId == null || s.actor?.can("documents.create") != true) return
    val context = LocalContext.current
    var photoUri by rememberSaveable { mutableStateOf<String?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { vm.upload(interventionId, it, false) }
    }
    val photo = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { success ->
        if (success) photoUri?.let { vm.upload(interventionId, Uri.parse(it), true) }
    }
    fun takePhoto() {
        val file = File(File(context.cacheDir, "photos").apply { mkdirs() }, "${UUID.randomUUID()}.jpg")
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        photoUri = uri.toString()
        try { photo.launch(uri) } catch (_: android.content.ActivityNotFoundException) { vm.message("Aucune application photo disponible") }
    }
    val cameraPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) takePhoto() else vm.message("Accès à l’appareil photo refusé")
    }
    val available = s.online && !s.busy && !interventionId.startsWith("temp_")
    Action("Ajouter un document (1 Mio maximum)", available) { pick.launch(arrayOf("*/*")) }
    Action("Prendre une photo (1 Mio maximum)", available) {
        if (androidx.core.content.ContextCompat.checkSelfPermission(context, android.Manifest.permission.CAMERA) == android.content.pm.PackageManager.PERMISSION_GRANTED)
            takePhoto() else cameraPermission.launch(android.Manifest.permission.CAMERA)
    }
}

@Composable private fun EquipmentScreen(vm: MaintViewModel, s: UiState, id: String, create: () -> Unit, intervention: (String) -> Unit) {
    LaunchedEffect(id) { vm.select("equipments", id) }
    Page {
        val item = s.selected?.takeIf { s.resourceKey == "equipments:$id" }
        if (item == null) { Text("Chargement de l’équipement…"); return@Page }
        Text(item.text("code").orEmpty()); Text(item.text("name").orEmpty(), style = MaterialTheme.typography.headlineSmall)
        Text(label(item.text("status")))
        if (s.actor?.can("interventions.create") == true) Action("Déclarer une intervention") { vm.equipmentForForm(id); create() }
        s.interventions.filter { it.text("equipment_id") == id }.forEach { itv ->
            EntityCard(itv, itv.text("title").orEmpty(), label(itv.text("status"))) { intervention(requireNotNull(itv.text("id"))) } }
        DocumentSection(vm, s, null)
    }
}

@Composable private fun SyncScreen(vm: MaintViewModel, s: UiState) {
    var review by remember { mutableStateOf<Pair<Operation, JsonObject?>?>(null) }
    var abandon by remember { mutableStateOf<Operation?>(null) }
    Page {
        Text("Synchronisation", style = MaterialTheme.typography.headlineSmall)
        Text("Dernier instantané : ${s.lastSync ?: "Aucun"}")
        Action("Synchroniser maintenant", s.online && !s.busy) { vm.sync() }
        if (!s.online) Text("Vos modifications seront envoyées au retour du réseau.")
        s.operations.forEach { op ->
            Card(Modifier.fillMaxWidth()) { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("${label(op.kind)} · ${label(op.state)}", style = MaterialTheme.typography.titleMedium)
                Text(op.occurredAt); Text(op.dependsOn ?: "Nouvelle intervention")
                op.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (op.state == "conflict") {
                    Action("Recharger et comparer", s.online && !s.busy) { vm.review(op) { fresh -> review = op to fresh } }
                    OutlinedButton({ abandon = op }) { Text("Abandonner") }
                }
            } }
        }
        if (s.operations.isEmpty()) Text("Aucune opération en attente.")
        if (s.session?.user?.role == "admin") {
            Text("Périmètre de travail", style = MaterialTheme.typography.titleMedium)
            Text("Le changement de site efface le cache local et sa file. Synchronisez ou abandonnez d’abord les opérations.")
            Action("Afficher les sites", !s.busy) { vm.sites() }
            s.sites.forEach { site -> Action(site.text("name").orEmpty(), !s.busy && s.operations.isEmpty()) { vm.selectSite(requireNotNull(site.text("id"))) } }
        }
    }
    review?.let { (op, fresh) -> AlertDialog(onDismissRequest = { review = null }, title = { Text("Comparer les versions") },
        text = { Column(Modifier.verticalScroll(rememberScrollState())) {
            val local = wire.parseToJsonElement(op.body).jsonObject
            Text("Modification locale :")
            Text(listOfNotNull(local.text("title"), local.text("description"), local.text("solution_report"),
                local.text("duration_minutes")?.let { "Temps total : $it min" }).joinToString("\n").ifBlank { label(op.kind) })
            Text("État actuel du serveur :"); Text(fresh?.let { "${label(it.text("status"))}\n${it.text("description").orEmpty()}\n${it.text("solution_report").orEmpty()}\nVersion : ${it.text("updated_at")}" }
                ?: "Création locale sans version distante")
            Text("Réessayer applique votre opération à la version rechargée. Un nouveau refus reste à résoudre.")
        } }, confirmButton = { TextButton({ review = null; vm.resolve(op.id, true, fresh?.text("updated_at")) }) { Text("Réessayer après rechargement") } },
        dismissButton = { TextButton({ review = null }) { Text("Annuler") } }) }
    abandon?.let { op -> AlertDialog(onDismissRequest = { abandon = null }, title = { Text("Abandonner cette opération") },
        text = { Text("L’opération sera abandonnée. Pour une création, ses actions dépendantes seront également abandonnées.") },
        confirmButton = { TextButton({ abandon = null; vm.resolve(op.id, false) }) { Text("Abandonner") } },
        dismissButton = { TextButton({ abandon = null }) { Text("Annuler") } }) }
}
