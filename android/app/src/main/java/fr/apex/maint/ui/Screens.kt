package fr.apex.maint.ui

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.Saver
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.compose.*
import fr.apex.maint.data.*
import fr.apex.maint.ui.theme.*
import kotlinx.serialization.json.*
import java.time.Instant

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

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun Page(modifier: Modifier = Modifier, refreshing: Boolean = false,
    onRefresh: (() -> Unit)? = null, content: @Composable ColumnScope.() -> Unit) {
    val body: @Composable () -> Unit = {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        Column(Modifier.widthIn(max = ApexLayout.formWidth).fillMaxWidth().verticalScroll(rememberScrollState()).padding(ApexLayout.padding),
            verticalArrangement = Arrangement.spacedBy(ApexLayout.padding), content = content)
        }
    }
    if (onRefresh != null) PullToRefreshBox(isRefreshing = refreshing, onRefresh = onRefresh,
        modifier = modifier.fillMaxSize().imePadding()) { body() }
    else Box(modifier.fillMaxSize().imePadding()) { body() }
}
@Composable internal fun Section(title: String, icon: ImageVector, content: @Composable ColumnScope.() -> Unit) {
    OutlinedCard(Modifier.fillMaxWidth(), colors = CardDefaults.outlinedCardColors(containerColor = LocalApexTokens.current.glass),
        border = BorderStroke(1.dp, LocalApexTokens.current.line)) {
        Column(Modifier.padding(ApexLayout.padding), verticalArrangement = Arrangement.spacedBy(ApexLayout.itemGap)) {
            Row(horizontalArrangement = Arrangement.spacedBy(ApexLayout.gap), verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, tint = MaterialTheme.colorScheme.primary)
                Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f).semantics { heading() })
            }
            content()
        }
    }
}
@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun EntityListPage(modifier: Modifier, state: LazyListState, refreshing: Boolean,
    refresh: () -> Unit, entries: List<JsonObject>, header: @Composable ColumnScope.() -> Unit,
    empty: @Composable ColumnScope.() -> Unit, card: @Composable (JsonObject) -> Unit) {
    PullToRefreshBox(isRefreshing = refreshing, onRefresh = refresh, modifier = modifier.fillMaxSize().imePadding()) {
        LazyColumn(state = state, modifier = Modifier.widthIn(max = ApexLayout.formWidth).fillMaxSize().align(Alignment.TopCenter),
            contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(ApexLayout.itemGap)) {
            item(key = "entete") { Column(verticalArrangement = Arrangement.spacedBy(ApexLayout.padding), content = header) }
            if (entries.isEmpty()) item(key = "vide") { Column(content = empty) }
            items(entries, key = { requireNotNull(it.text("id")) }) { card(it) }
        }
    }
}
@Composable internal fun rememberDraft(vm: MaintViewModel, key: String, initial: String): MutableState<String> {
    val s by vm.state.collectAsStateWithLifecycle()
    val context = draftContext(s.server, s.actor?.scope, key, s.draftRevisions)
    val cipher = vm.container.secure.cipher
    val saver = Saver<MutableState<String>, String>(
        save = { cipher.encrypt(context, it.value) },
        restore = { runCatching { mutableStateOf(cipher.decrypt(context, it)) }.getOrNull() })
    val draft = rememberSaveable(context, saver = saver) { mutableStateOf(initial) }
    var baseline by remember(context) { mutableStateOf(initial) }
    LaunchedEffect(context, initial) {
        if (draft.value == baseline) draft.value = initial
        baseline = initial
    }
    return draft
}
@OptIn(ExperimentalLayoutApi::class)
@Composable private fun Badges(item: JsonObject) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(ApexLayout.itemGap), verticalArrangement = Arrangement.spacedBy(ApexLayout.gap)) {
        listOfNotNull(item.text("status"), item.text("priority"), item.text("machine_state")).forEach { value ->
            val colors = LocalApexTokens.current.badge(value)
            Surface(color = colors.background, contentColor = colors.text,
                shape = RoundedCornerShape(ApexLayout.badgeRadius), border = BorderStroke(1.dp, colors.border)) {
                Text(label(value), modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                    textDecoration = if (value == "cancelled") TextDecoration.LineThrough else null,
                    style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}
@Composable internal fun Field(label: String, value: String, change: (String) -> Unit,
    secret: Boolean = false, lines: Int = 1, number: Boolean = false, limit: Int? = null) {
    var visible by remember { mutableStateOf(false) }
    val length = value.codePointCount(0, value.length)
    OutlinedTextField(value, change, label = { Text(label) }, modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(ApexLayout.fieldRadius),
        colors = OutlinedTextFieldDefaults.colors(focusedContainerColor = LocalApexTokens.current.field,
            unfocusedContainerColor = LocalApexTokens.current.field, errorContainerColor = LocalApexTokens.current.field,
            unfocusedBorderColor = LocalApexTokens.current.line),
        minLines = lines, maxLines = if (lines == 1) 1 else 6, singleLine = lines == 1,
        isError = limit != null && length > limit,
        supportingText = if (limit != null) ({ Text("$length / $limit caractères") }) else null,
        keyboardOptions = KeyboardOptions(keyboardType = when { secret -> KeyboardType.Password; number -> KeyboardType.Number; else -> KeyboardType.Text }),
        trailingIcon = if (secret) ({ IconButton({ visible = !visible }) {
            Icon(if (visible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                if (visible) "Masquer le mot de passe" else "Afficher le mot de passe")
        } }) else null,
        visualTransformation = if (secret && !visible) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None)
}
@Composable internal fun Action(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    Button(onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch)) { Text(text) }
}
@Composable private fun NavigationEntry(title: String, icon: ImageVector, selected: Boolean,
    modifier: Modifier = Modifier, click: () -> Unit) {
    val tokens = LocalApexTokens.current
    Column(modifier.padding(4.dp).background(if (selected) tokens.selected else Color.Transparent,
        RoundedCornerShape(ApexLayout.radius)).selectable(selected, role = Role.Tab, onClick = click)
        .heightIn(min = ApexLayout.touch).padding(horizontal = 4.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(ApexLayout.unit)) {
        Icon(icon, null, tint = if (selected) tokens.accent else tokens.muted)
        Text(title, style = MaterialTheme.typography.labelMedium, textAlign = TextAlign.Center,
            color = if (selected) tokens.text else tokens.muted)
    }
}
@Composable private fun ThemeSetting(vm: MaintViewModel) {
    val choice by vm.theme.collectAsStateWithLifecycle()
    ThemeOptions(choice ?: ThemeChoice.DARK) { vm.theme(it) }
}
@Composable internal fun ThemeOptions(choice: ThemeChoice, choose: (ThemeChoice) -> Unit) {
    Column(Modifier.selectableGroup()) {
        Text("Thème", style = MaterialTheme.typography.titleMedium)
        ThemeChoice.entries.forEach { option ->
            Row(Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch)
                .selectable(choice == option, role = Role.RadioButton, onClick = { choose(option) })
                .padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                RadioButton(selected = choice == option, onClick = null)
                Spacer(Modifier.width(8.dp))
                Text(option.title, modifier = Modifier.weight(1f))
            }
        }
        Text("Système suit le mode d’affichage de l’appareil.", style = MaterialTheme.typography.bodyMedium,
            color = LocalApexTokens.current.muted)
    }
}
@Composable private fun Picker(title: String, selected: String, choices: List<Pair<String, String>>, choose: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        ApexOutlinedButton({ expanded = true }, modifier = Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch)) {
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
    val nav = key(s.actor?.scope) { rememberNavController() }
    val terrainList = key(s.actor?.scope) { rememberLazyListState() }
    val stockList = key(s.actor?.scope) { rememberLazyListState() }
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val home = when {
        s.actor?.can("interventions.view") == true -> "terrain"
        s.actor?.can("parts.view") == true -> "stock"
        else -> "sync"
    }
    fun destination(id: String) {
        nav.navigate(id) {
            popUpTo(nav.graph.startDestinationId) { saveState = true }
            launchSingleTop = true; restoreState = true
        }
    }
    val window = currentWindowAdaptiveInfo().windowSizeClass
    val rail = window.isWidthAtLeastBreakpoint(ApexLayout.railBreakpoint)
    // Deux volets dès la classe « expanded » (840 dp, seuil le plus large connu par défaut) ;
    // à police agrandie (130 % et plus), un seul volet pour garder des formulaires lisibles.
    val wide = LocalDensity.current.fontScale < 1.3f && window.isWidthAtLeastBreakpoint(ApexLayout.detailBreakpoint)
    var settings by rememberSaveable { mutableStateOf(false) }
    var logout by remember { mutableStateOf(false) }
    val snack = remember { SnackbarHostState() }
    LaunchedEffect(s.message) { s.message?.let { snack.showSnackbar(it); vm.message(null) } }
    Scaffold(containerColor = Color.Transparent, snackbarHost = { SnackbarHost(snack) }, topBar = {
        TopAppBar(colors = TopAppBarDefaults.topAppBarColors(containerColor = LocalApexTokens.current.bar),
            title = { Text("Apex Maint", style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis) }, navigationIcon = {
            if (s.actor != null && route in setOf("terrain/{id}", "stock/{id}", "equipement/{id}", "creation"))
                IconButton({ if (!nav.popBackStack()) nav.navigate("terrain") }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Retour à la liste") }
        }, actions = {
            IconButton({ settings = true }) { Icon(Icons.Default.Settings, "Réglages et compte") }
            if (s.actor != null && !s.expired && s.session?.must_change_password != true && s.session?.user?.must_change_password != true) IconButton({ vm.sync() }, enabled = !s.busy) {
                Icon(Icons.Default.Sync, "Synchroniser puis actualiser les données")
            }
            if (s.session != null) IconButton({ logout = true }) { Icon(Icons.Default.Logout, "Se déconnecter") }
        })
    }) { inset ->
        Column(Modifier.padding(inset).consumeWindowInsets(inset).fillMaxSize()) {
            Box(Modifier.fillMaxWidth().height(4.dp)) {
                if (s.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
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
                    if (s.operations.isNotEmpty() || !s.online) Surface(color = MaterialTheme.colorScheme.primaryContainer) {
                        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                            val conflicts = s.operations.count { it.state == "conflict" }
                            Text(listOfNotNull(if (!s.online) "Hors ligne" else null,
                                if (s.operations.isNotEmpty()) operationCount(s.operations.size) else "Consultation du cache local",
                                if (conflicts > 0) "$conflicts à résoudre" else null).joinToString(" · "),
                                Modifier.weight(1f).padding(vertical = 12.dp))
                            IconButton({ destination("sync") }) { Icon(Icons.Default.CloudUpload, "Voir la file et les conflits") }
                        }
                    }
                    val entries = buildList {
                        if (s.actor!!.can("interventions.view")) add(Triple("terrain", "Terrain", Icons.Default.Build))
                        if (s.actor!!.can("parts.view")) add(Triple("stock", "Pièces", Icons.Default.Inventory2))
                        if (s.actor!!.can("equipments.view")) add(Triple("scanner", "Scanner", Icons.Default.QrCodeScanner))
                        add(Triple("sync", "Synchronisation", Icons.Default.Sync))
                    }
                    val currentId = nav.currentBackStackEntryAsState().value?.arguments?.getString("id")
                    LaunchedEffect(currentId, s.aliases) {
                        s.aliases[currentId]?.let { real -> nav.navigate("terrain/$real") {
                            popUpTo("terrain/{id}") { inclusive = true }
                        } }
                    }
                    Row(Modifier.weight(1f)) {
                        if (rail) NavigationRail(modifier = Modifier.width(ApexLayout.railWidth), containerColor = LocalApexTokens.current.nav) {
                            Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                            entries.forEach { (id, title, icon) -> NavigationEntry(title, icon,
                                route?.startsWith(id) == true, Modifier.fillMaxWidth()) { destination(id) } }
                            }
                        }
                        NavHost(nav, startDestination = home, modifier = Modifier.weight(1f)) {
                            composable("terrain") { Terrain(vm, s, wide, null, terrainList, { nav.navigate("terrain/$it") }, { nav.navigate("creation") }, { nav.navigate("equipement/$it") }) }
                            composable("terrain/{id}") { entry -> Terrain(vm, s, wide, entry.arguments?.getString("id"), terrainList,
                                { nav.navigate("terrain/$it") }, { nav.navigate("creation") }, { nav.navigate("equipement/$it") }) }
                            composable("creation") { CreateScreen(vm, s, { nav.navigate("scanner") }) { nav.navigate("terrain/$it") } }
                            composable("stock") { StockScreen(vm, s, wide, null, stockList) { nav.navigate("stock/$it") } }
                            composable("stock/{id}") { entry -> StockScreen(vm, s, wide, entry.arguments?.getString("id"), stockList) { nav.navigate("stock/$it") } }
                            composable("scanner") {
                                if (s.actor!!.can("equipments.view")) ScannerScreen(vm) { value -> vm.scan(value) { collection, id ->
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
                    if (!rail) NavigationBar(containerColor = LocalApexTokens.current.nav) {
                        entries.forEach { (id, title, icon) -> NavigationEntry(if (id == "sync") "Synchro" else title, icon,
                            route?.startsWith(id) == true, Modifier.weight(1f)) { destination(id) } }
                    }
                }
            }
        }
    }
    if (settings) AlertDialog(onDismissRequest = { settings = false }, title = { Text("Réglages et compte") },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(ApexLayout.padding)) {
            s.session?.user?.display_name?.let { Text(it, style = MaterialTheme.typography.titleMedium) }
            Text(s.server ?: "Serveur non configuré", color = LocalApexTokens.current.muted)
            ThemeSetting(vm)
        } }, confirmButton = { ApexTextButton({ settings = false }) { Text("Fermer") } })
    if (logout) AlertDialog(onDismissRequest = { logout = false }, title = { Text("Se déconnecter") },
        text = { Text("Le cache sera effacé. " + when (s.operations.size) {
            0 -> "Aucune opération à envoyer."
            1 -> "Une opération non synchronisée sera perdue."
            else -> "${s.operations.size} opérations non synchronisées seront perdues."
        }) },
        confirmButton = { ApexTextButton({ logout = false; vm.logout() }) { Text("Se déconnecter") } },
        dismissButton = { ApexTextButton({ logout = false }) { Text("Annuler") } })
}

@Composable private fun ServerScreen(vm: MaintViewModel, s: UiState) {
    var url by rememberSaveable { mutableStateOf("") }
    Page {
        Text("Configurer le serveur", style = MaterialTheme.typography.headlineSmall)
        Text("Indiquez l’adresse complète fournie par votre administrateur, avec https:// et le port éventuel.")
        Field("Adresse HTTPS du serveur", url, { url = it })
        Text("Exemple : https://maint.atelier.local:8443/\nUtilisez l’adresse racine, sans /api ni autre chemin.", color = LocalApexTokens.current.muted, style = MaterialTheme.typography.bodyMedium)
        Section("Certificat de l’instance", Icons.Default.Security) {
            Text("Si votre instance utilise une autorité privée, faites vérifier puis installer son certificat CA dans les paramètres de sécurité Android. L’adresse saisie doit correspondre au certificat.")
        }
        Action("Enregistrer le serveur", !s.busy && url.isNotBlank()) { vm.server(url) }
        ThemeSetting(vm)
    }
}
@Composable private fun LoginScreen(vm: MaintViewModel, s: UiState) {
    var user by rememberDraft(vm, "connexion:identifiant", "")
    var password by remember { mutableStateOf("") }
    val context = LocalContext.current
    Page {
        Text("Connexion", style = MaterialTheme.typography.headlineSmall)
        Text(s.server.orEmpty(), color = LocalApexTokens.current.muted)
        if (s.expired) Text("Votre session a expiré. Reconnectez-vous avec le même compte pour conserver la file.")
        Field("Identifiant", user, { user = it })
        Field("Mot de passe", password, { password = it }, secret = true)
        Action("Se connecter", !s.busy && user.isNotBlank() && password.isNotEmpty()) {
            vm.login(user, password); password = ""
        }
        ApexOutlinedButton({ context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(s.server + "api/tls/ca.pem"))) }) {
            Text("Télécharger l’autorité dans le navigateur")
        }
        Text("Après vérification de son empreinte, installez cette autorité comme certificat CA. Un premier transfert peut nécessiter votre administrateur.")
        if (s.session == null) {
            var edit by remember { mutableStateOf(false) }
            ApexTextButton({ edit = !edit }) { Text("Modifier l’URL") }
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

@Composable private fun Terrain(vm: MaintViewModel, s: UiState, wide: Boolean, id: String?, listState: LazyListState,
    select: (String) -> Unit, create: () -> Unit, equipment: (String) -> Unit) {
    if (s.actor?.can("interventions.view") != true) { Page { Text("Consultation des interventions non autorisée") }; return }
    LaunchedEffect(id) { if (id != null) vm.select("interventions", id) }
    val mine by vm.mine.collectAsStateWithLifecycle()
    val status by vm.status.collectAsStateWithLifecycle()
    val entries = s.interventions.filter { item -> (status.isEmpty() || item.text("status") == status) &&
        (!mine || item.list("technicians").any { it.text("user_id") == s.actor?.userId }) }
    Row(Modifier.fillMaxSize()) {
        if (wide || id == null) EntityListPage(Modifier.weight(if (wide) 0.85f else 1f), listState, s.refreshing,
            refresh = { if (s.online && !s.busy) vm.sync() else if (!s.online) vm.message("Le rafraîchissement nécessite le réseau") },
            entries = entries, header = {
                Text("Interventions", style = MaterialTheme.typography.headlineSmall)
                Row(Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch).toggleable(mine, role = Role.Checkbox,
                    onValueChange = { vm.filterMine(it); if (!it && s.online && !s.busy) vm.siteList() }),
                    verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(mine, null); Text("Mes interventions")
                }
                Picker("Statut", status, listOf("" to "Tous") + listOf("open", "in_progress", "completed", "cancelled").map { it to label(it) }) { vm.filterStatus(it) }
                ApexOutlinedButton({ vm.siteList() }, enabled = s.online && !s.busy, modifier = Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch)) {
                    Icon(Icons.Default.Refresh, null); Spacer(Modifier.width(8.dp)); Text("Recharger les interventions du site")
                }
                if (s.actor?.can("interventions.create") == true) Action("Déclarer une intervention", !s.busy, create)
            }, empty = {
                Section("Aucune intervention", Icons.Default.Search) {
                    Text(if (mine) "Aucune intervention ne vous est affectée dans ce filtre. Consultez celles du site pour en prendre une en charge."
                        else "Aucune intervention ne correspond au filtre. Retirez le filtre ou rechargez les données du site.")
                    if (mine) Action("Voir les interventions du site") { vm.filterMine(false); if (s.online && !s.busy) vm.siteList() }
                    if (status.isNotEmpty()) ApexTextButton({ vm.filterStatus("") }) { Text("Afficher tous les statuts") }
                    if (!s.online) Text("Hors ligne : seules les données déjà chargées sont disponibles.")
                }
            }) { item -> EntityCard(item, item.text("title").orEmpty(), "", selected = item.text("id") == id) {
                select(requireNotNull(item.text("id")))
            } }
        if (wide || id != null) {
            if (wide) VerticalDivider()
            if (id == null) Page(Modifier.weight(1f)) { Text("Sélectionnez une intervention pour consulter ses informations et ses documents.") }
            else InterventionScreen(vm, s, id, Modifier.weight(1f), select, equipment)
        }
    }
}
@Composable internal fun EntityCard(item: JsonObject, title: String, subtitle: String,
    selected: Boolean = false, click: () -> Unit) {
    Card(onClick = click, modifier = Modifier.fillMaxWidth().heightIn(min = 64.dp),
        colors = CardDefaults.cardColors(containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer
            else LocalApexTokens.current.glass),
        border = BorderStroke(1.dp, if (selected) LocalApexTokens.current.accent else LocalApexTokens.current.line)) {
        Column(Modifier.padding(ApexLayout.padding), verticalArrangement = Arrangement.spacedBy(ApexLayout.gap)) {
            Text(item.text("code") ?: item.text("reference").orEmpty(), color = LocalApexTokens.current.muted, style = MaterialTheme.typography.labelLarge)
            Text(title, style = MaterialTheme.typography.titleMedium)
            if (item.text("status") != null || item.text("priority") != null) Badges(item) else Text(subtitle, color = LocalApexTokens.current.muted)
        }
    }
}
@Composable private fun InterventionScreen(vm: MaintViewModel, s: UiState, id: String, modifier: Modifier,
    select: (String) -> Unit, equipment: (String) -> Unit) {
    val item = s.selected?.takeIf { s.resourceKey == "interventions:$id" }
    Page(modifier) {
        if (item == null) {
            Text(if (s.busy) "Chargement de l’intervention…" else "Intervention indisponible sur cet appareil.")
            if (!s.busy) Action("Réessayer", s.online) { vm.select("interventions", id) }
            return@Page
        }
        Text(item.text("code").orEmpty(), color = LocalApexTokens.current.muted, style = MaterialTheme.typography.labelLarge)
        Text(item.text("title").orEmpty(), style = MaterialTheme.typography.headlineSmall)
        Badges(item)
        var description by rememberDraft(vm, "$id:description", item.text("description").orEmpty())
        var report by rememberDraft(vm, "$id:report", item.text("solution_report").orEmpty())
        var time by rememberDraft(vm, "$id:time", item.list("technicians").find { it.text("user_id") == s.actor?.userId }?.text("duration_minutes") ?: "0")
        var complete by rememberSaveable(id) { mutableStateOf(false) }
        val assigned = item.list("technicians").any { it.text("user_id") == s.actor?.userId }
        val canAct = s.actor?.can("interventions.edit") == true && item.text("status") in setOf("open", "in_progress")
        val editable = canAct && (s.session?.user?.role != "technicien" || assigned)
        val completionProblem = TerrainRules.completionProblem(item, report, s.children)
        if (canAct) Section("Actions principales", Icons.Default.Build) {
            if (item.text("status") == "open") {
                Action("Prendre en charge", !s.busy) { vm.mutate(id, "take_intervention", obj()) }
            } else if (editable) {
                Action("Clôturer l’intervention", !s.busy && completionProblem == null) { complete = true }
                completionProblem?.let { Text(it, color = LocalApexTokens.current.muted, style = MaterialTheme.typography.bodyMedium) }
            } else {
                Text("Vous devez être affecté à cette intervention pour la modifier. Contactez votre responsable.")
            }
        }
        Section("Informations", Icons.Default.Info) {
            Text("Déclarée le ${frenchDate(item.text("reported_at"))}")
            item.text("started_at")?.let { Text("Début : ${frenchDate(it)}") }
            item.text("completed_at")?.let { Text("Clôture : ${frenchDate(it)}") }
            Text("Type : ${label(item.text("type"))} · Machine : ${label(item.text("machine_state"))}")
            item.text("equipment_id")?.let { equipmentId ->
                val equipmentItem = s.equipments.find { it.text("id") == equipmentId }
                if (s.actor?.can("equipments.view") == true) ApexOutlinedButton({ equipment(equipmentId) }) {
                    Icon(Icons.Default.PrecisionManufacturing, null); Spacer(Modifier.width(8.dp))
                    Text(equipmentItem?.let { "${it.text("code")} · ${it.text("name")}" } ?: "Voir l’équipement et ses documents")
                }
            }
            if (editable) Field("Description", description, { description = it }, lines = 3, limit = 4096)
            else Text(item.text("description")?.takeIf { it.isNotBlank() } ?: "Aucune description.")
        }
        Section("Compte rendu", Icons.Default.Assignment) {
            if (editable) {
                Field("Travaux réalisés", report, { report = it }, lines = 3, limit = 16384)
                Action("Enregistrer la description et le compte rendu", !s.busy && TerrainRules.inputProblem(obj("description" to description.j(), "solution_report" to report.j())) == null) { vm.mutate(id, "patch_intervention",
                    obj("description" to description.j(), "solution_report" to report.j())) }
            } else Text(item.text("solution_report")?.takeIf { it.isNotBlank() } ?: "Aucun compte rendu.")
        }
        Section("Temps passé", Icons.Default.Schedule) {
            Text("Temps total : ${frenchDuration(item.text("duration_minutes")?.toIntOrNull() ?: 0)}")
            if (editable && assigned) {
                Field("Mon temps total en minutes", time, { time = it }, number = true)
                Text("Cette valeur remplace votre temps déjà saisi.", color = LocalApexTokens.current.muted, style = MaterialTheme.typography.bodyMedium)
                Action("Enregistrer mon temps", !s.busy && (time.toIntOrNull() ?: -1) >= 0) {
                    vm.mutate(id, "record_time", obj("duration_minutes" to requireNotNull(time.toIntOrNull()).j())) }
            }
            item.list("technicians").forEach { tech -> Text("${tech.text("display_name") ?: s.session?.user?.display_name?.takeIf { tech.text("user_id") == s.actor?.userId } ?: "Technicien"} : ${frenchDuration(tech.text("duration_minutes")?.toIntOrNull() ?: 0)}") }
        }
        Section("Pièces consommées", Icons.Default.Inventory2) {
            if (s.consumed.isEmpty()) Text(if (s.online) "Aucune pièce consommée chargée." else "Consultation et sorties de pièces disponibles en ligne.")
            s.consumed.forEach { Text("${it.text("part_reference")} · ${it.text("part_name")} : ${it.text("quantity")} (${it.text("location_name")})") }
            if (editable && s.online && !id.startsWith("temp_") && s.actor?.can("stock.movements") == true) ConsumeForm(vm, s, id)
        }
        Section("Documents", Icons.Default.Description) { DocumentSection(vm, s, id) }
        Section("Sous-interventions", Icons.Default.AccountTree) {
            if (s.children.isEmpty()) Text("Aucune sous-intervention chargée.")
            s.children.forEach { child -> EntityCard(child, child.text("title").orEmpty(), label(child.text("status"))) { select(requireNotNull(child.text("id"))) } }
        }
        if (complete) AlertDialog(onDismissRequest = { complete = false }, title = { Text("Confirmer la clôture") },
            text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(ApexLayout.itemGap)) {
                Text("L’intervention sera clôturée avec ce compte rendu. Cette action sera envoyée au serveur dès que possible.")
                Text(report)
                if (description != item.text("description").orEmpty()) Text("La description modifiée doit être enregistrée séparément avant la clôture.")
            } }, confirmButton = { ApexTextButton({ complete = false
                vm.mutate(id, "complete_intervention", obj("solution_report" to report.j())) }, enabled = !s.busy && editable && completionProblem == null) { Text("Clôturer") } },
            dismissButton = { ApexTextButton({ complete = false }) { Text("Annuler") } })
    }
}
@Composable private fun CreateScreen(vm: MaintViewModel, s: UiState, scan: () -> Unit, done: (String) -> Unit) {
    var title by rememberDraft(vm, "creation:title", ""); var description by rememberDraft(vm, "creation:description", "")
    var search by remember { mutableStateOf("") }; val equipment = s.formEquipment
    var priority by rememberSaveable { mutableStateOf("medium") }; var machine by rememberSaveable { mutableStateOf("FONCTIONNEL") }
    var type by rememberSaveable { mutableStateOf("corrective") }; var report by rememberDraft(vm, "creation:report", "")
    var minutes by rememberDraft(vm, "creation:minutes", "0")
    Page {
        Text("Déclarer une intervention", style = MaterialTheme.typography.headlineSmall)
        Section("Déclaration", Icons.Default.Assignment) {
        Field("Titre obligatoire", title, { title = it }, limit = 128); Field("Description", description, { description = it }, lines = 3, limit = 4096)
        Picker("Type", type, listOf("corrective", "preventive", "hors_equipement", "a_posteriori").map { it to label(it) }) { type = it }
        Picker("Priorité", priority, listOf("low", "medium", "high", "critical").map { it to label(it) }) { priority = it }
        Picker("État machine", machine, listOf("FONCTIONNEL", "ARRET", "DEGRADE").map { it to label(it) }) { machine = it }
        }
        if (type != "hors_equipement") Section("Équipement", Icons.Default.PrecisionManufacturing) {
        if (s.actor?.can("equipments.view") == true) Action("Scanner un équipement", onClick = scan)
        Field("Rechercher un équipement", search, { search = it })
        Text("Équipement : " + (s.equipments.find { it.text("id") == equipment }?.let { "${it.text("code")} · ${it.text("name")}" } ?: "Non sélectionné"))
        if (equipment != null) ApexTextButton({ vm.equipmentForForm(null) }) { Text("Retirer l’équipement") }
        if (search.isNotBlank()) s.equipments.filter { "${it.text("code")} ${it.text("name")}".contains(search, true) }.take(30).forEach { e ->
            ApexOutlinedButton({ vm.equipmentForForm(e.text("id")); search = "" }, Modifier.fillMaxWidth()) { Text("${e.text("code")} · ${e.text("name")}") }
        }
        if (equipment == null) Text("Choisissez un équipement, ou le type Hors équipement pour une déclaration sans machine.", color = LocalApexTokens.current.muted, style = MaterialTheme.typography.bodyMedium)
        }
        if (type == "a_posteriori") Section("Travaux déjà réalisés", Icons.Default.AssignmentTurnedIn) {
            Field("Compte rendu obligatoire", report, { report = it }, lines = 3, limit = 16384)
            Field("Durée en minutes", minutes, { minutes = it }, number = true)
        }
        val valid = title.isNotBlank() && (equipment != null || type == "hors_equipement") &&
            TerrainRules.inputProblem(obj("title" to title.j(), "description" to description.j(), "solution_report" to report.j())) == null &&
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
            vm.create(JsonObject(fields)) { id ->
                title = ""; description = ""; report = ""; minutes = "0"; vm.equipmentForForm(null); done(id)
            }
        }
        if (!s.online) Text("Cette déclaration sera synchronisée au retour du réseau.")
    }
}

@Composable private fun StockScreen(vm: MaintViewModel, s: UiState, wide: Boolean, id: String?, listState: LazyListState, select: (String) -> Unit) {
    if (s.actor?.can("parts.view") != true) { Page { Text("Consultation des pièces non autorisée") }; return }
    LaunchedEffect(id) { if (id != null) vm.select("parts", id) }
    var q by rememberDraft(vm, "stock:search", "")
    val parts = s.parts.filter { "${it.text("reference")} ${it.text("name")}".contains(q, true) }
    Row(Modifier.fillMaxSize()) {
        if (wide || id == null) EntityListPage(Modifier.weight(if (wide) 0.85f else 1f), listState, s.refreshing,
            refresh = { if (s.online && !s.busy) vm.sync() else if (!s.online) vm.message("Le rafraîchissement nécessite le réseau") },
            entries = parts, header = {
                Text("Catalogue des pièces", style = MaterialTheme.typography.headlineSmall)
                Field("Rechercher une pièce", q, { q = it })
                Action("Actualiser le catalogue", s.online && !s.busy) { vm.catalogue() }
            }, empty = {
                Text(if (q.isBlank()) "Aucune pièce chargée. Actualisez le catalogue en ligne." else "Aucune pièce trouvée. Essayez une autre référence ou un autre nom.")
            }) { part -> EntityCard(part, part.text("name").orEmpty(),
                "Disponible : ${part.text("total_available") ?: part.text("available") ?: "—"}", selected = part.text("id") == id) {
                select(requireNotNull(part.text("id")))
            } }
        if (wide || id != null) {
            if (wide) VerticalDivider()
            Page(Modifier.weight(1f)) {
                val part = s.selected?.takeIf { s.resourceKey == "parts:$id" }
                if (part == null) {
                    Text(if (id == null) "Sélectionnez une pièce" else if (s.busy) "Chargement de la pièce…" else "Pièce indisponible sur cet appareil.")
                    if (id != null && !s.busy) Action("Réessayer", s.online) { vm.select("parts", id) }
                } else {
                    Text(part.text("reference").orEmpty(), style = MaterialTheme.typography.labelLarge)
                    Text(part.text("name").orEmpty(), style = MaterialTheme.typography.headlineSmall)
                    Section("Disponibilité", Icons.Default.Inventory2) {
                        Text("Disponible : ${part.text("total_available") ?: part.text("available") ?: "—"}")
                        Text(part.text("description").orEmpty())
                        if (!s.online) Text("Quantités mémorisées : vérifiez-les en ligne avant toute sortie.")
                    }
                    Section("Stock par emplacement", Icons.Default.Warehouse) {
                        if (part.list("locations").isEmpty()) Text("Aucun emplacement chargé. Rechargez la fiche en ligne.")
                        part.list("locations").forEach { location ->
                            Text("${location.text("location_code")} · ${location.text("location_name")}", style = MaterialTheme.typography.titleSmall)
                            Text("${location.text("quantity")} en stock · ${location.text("available_quantity")} disponibles · ${location.text("reserved_quantity")} réservés")
                        }
                    }
                }
            }
        }
    }
}
@Composable private fun ConsumeForm(vm: MaintViewModel, s: UiState, interventionId: String) {
    if (s.actor?.can("interventions.edit") != true) return
    if (s.actor?.can("parts.view") != true) { Text("La sortie de pièce nécessite aussi l’accès au catalogue."); return }
    var open by rememberSaveable(interventionId) { mutableStateOf(false) }
    var selectedPart by rememberSaveable(interventionId) { mutableStateOf<String?>(null) }
    var part by remember(interventionId) { mutableStateOf<JsonObject?>(null) }
    var location by rememberSaveable(interventionId) { mutableStateOf("") }
    var quantity by rememberDraft(vm, "$interventionId:quantite", "1")
    LaunchedEffect(open, selectedPart) {
        part = null
        if (open) selectedPart?.let { id -> vm.part(id) { if (selectedPart == id) part = it } }
    }
    ApexOutlinedButton({ open = !open; if (open) vm.catalogue() }, enabled = !s.busy) { Text("Sortir une pièce") }
    if (open) {
        var search by remember { mutableStateOf("") }
        Field("Rechercher la pièce à sortir", search, { search = it })
        s.parts.filter { "${it.text("reference")} ${it.text("name")}".contains(search, true) }.take(20).forEach { candidate ->
            ApexOutlinedButton({ location = ""; selectedPart = requireNotNull(candidate.text("id")) }, enabled = !s.busy) { Text("${candidate.text("reference")} · ${candidate.text("name")}") }
        }
        // Les détails de pièce sont chargés séparément pour conserver la fiche intervention visible.
        val locations = part?.list("locations").orEmpty()
        if (part != null) {
            Picker("Emplacement", location, locations.map { requireNotNull(it.text("location_id")) to
                "${it.text("location_name")} (${it.text("available_quantity")} disponibles)" }) { location = it }
            Field("Quantité", quantity, { quantity = it }, number = true)
            Action("Confirmer la sortie", !s.busy && location.isNotBlank() && (quantity.toIntOrNull() ?: 0) > 0) {
                vm.consume(interventionId, requireNotNull(part?.text("id")), location, requireNotNull(quantity.toIntOrNull())); open = false
            }
        }
    }
}

@Composable private fun ColumnScope.DocumentSection(vm: MaintViewModel, s: UiState, interventionId: String?) {
    if (s.actor?.can("documents.view") == true) {
        if (s.documents.isEmpty()) Text(if (s.online) "Aucun document chargé pour cette fiche." else "Connectez-vous au réseau pour consulter les documents.")
        s.documents.forEach { doc -> ApexOutlinedButton({ vm.openDocument(doc) }, enabled = s.online && !s.busy,
            modifier = Modifier.fillMaxWidth().heightIn(min = ApexLayout.touch)) {
            Icon(Icons.Default.Description, null); Spacer(Modifier.width(8.dp)); Text(doc.text("title").orEmpty())
        } }
        s.resourceKey?.let { key -> ApexOutlinedButton({ vm.select(key.substringBefore(':'), key.substringAfter(':')) },
            enabled = s.online && !s.busy && !key.substringAfter(':').startsWith("temp_")) { Text("Recharger les documents") } }
    } else Text("Votre compte ne permet pas de consulter les documents.")
    if (interventionId == null || s.actor?.can("documents.create") != true) return
    val context = LocalContext.current
    var photoUri by rememberSaveable { mutableStateOf<String?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { vm.upload(interventionId, it, false) }
    }
    val photo = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { success ->
        if (success) photoUri?.let { vm.photoCaptured(interventionId, Uri.parse(it)) }
        else photoUri?.let { context.contentResolver.delete(Uri.parse(it), null, null) }
        photoUri = null
    }
    fun takePhoto() {
        val file = TemporaryFiles(context.cacheDir).create("jpg")
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        photoUri = uri.toString()
        try { photo.launch(uri) } catch (e: Exception) {
            file.delete(); photoUri = null
            vm.message(if (e is android.content.ActivityNotFoundException) "Aucune application photo disponible" else "Capture indisponible")
        }
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
        if (item == null) {
            Text(if (s.busy) "Chargement de l’équipement…" else "Équipement indisponible sur cet appareil.")
            if (!s.busy) Action("Réessayer", s.online) { vm.select("equipments", id) }
            return@Page
        }
        Text(item.text("code").orEmpty()); Text(item.text("name").orEmpty(), style = MaterialTheme.typography.headlineSmall)
        Badges(item)
        item.text("description")?.takeIf { it.isNotBlank() }?.let { Text(it) }
        if (s.actor?.can("interventions.create") == true) Action("Déclarer une intervention") { vm.equipmentForForm(id); create() }
        Section("Documents de l’équipement", Icons.Default.Description) { DocumentSection(vm, s, null) }
        Text("Interventions de l’équipement", style = MaterialTheme.typography.titleMedium)
        val interventions = s.interventions.filter { it.text("equipment_id") == id }
        if (interventions.isEmpty()) Text("Aucune intervention chargée pour cet équipement.")
        interventions.forEach { itv ->
            EntityCard(itv, itv.text("title").orEmpty(), label(itv.text("status"))) { intervention(requireNotNull(itv.text("id"))) } }
    }
}

@Composable private fun SyncScreen(vm: MaintViewModel, s: UiState) {
    var review by remember { mutableStateOf<Pair<Operation, JsonObject?>?>(null) }
    var abandon by remember { mutableStateOf<Operation?>(null) }
    Page {
        Text("Synchronisation", style = MaterialTheme.typography.headlineSmall)
        Text("Dernier instantané : ${if (s.lastSync == null) "Aucun" else frenchDate(s.lastSync)}")
        Action("Synchroniser maintenant", !s.busy) { vm.sync() }
        if (!s.online) Text("Vos modifications seront envoyées au retour du réseau.")
        s.operations.forEach { op ->
            OutlinedCard(Modifier.fillMaxWidth(), colors = CardDefaults.outlinedCardColors(containerColor = LocalApexTokens.current.glass), border = BorderStroke(1.dp, LocalApexTokens.current.line)) { Column(Modifier.padding(ApexLayout.padding), verticalArrangement = Arrangement.spacedBy(ApexLayout.gap)) {
                Text("${label(op.kind)} · ${label(op.state)}", style = MaterialTheme.typography.titleMedium)
                Text(frenchDate(op.occurredAt)); Text(s.interventions.find { it.text("id") == op.dependsOn }?.text("title") ?: "Nouvelle intervention")
                op.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (op.state == "conflict") {
                    Action("Recharger et comparer", s.online && !s.busy) { vm.review(op) { fresh -> review = op to fresh } }
                    ApexOutlinedButton({ abandon = op }) { Text("Abandonner") }
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
        } }, confirmButton = { ApexTextButton({ review = null; vm.resolve(op.id, true, fresh?.text("updated_at")) }) { Text("Réessayer après rechargement") } },
        dismissButton = { ApexTextButton({ review = null }) { Text("Annuler") } }) }
    abandon?.let { op -> AlertDialog(onDismissRequest = { abandon = null }, title = { Text("Abandonner cette opération") },
        text = { Text("L’opération sera abandonnée. Pour une création, ses actions dépendantes seront également abandonnées.") },
        confirmButton = { ApexTextButton({ abandon = null; vm.resolve(op.id, false) }) { Text("Abandonner") } },
        dismissButton = { ApexTextButton({ abandon = null }) { Text("Annuler") } }) }
}
