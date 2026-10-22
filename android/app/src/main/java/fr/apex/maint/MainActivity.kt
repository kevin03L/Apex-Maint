package fr.apex.maint

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.apex.maint.ui.theme.ApexTheme
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewmodel.CreationExtras
import androidx.lifecycle.viewmodel.compose.viewModel
import fr.apex.maint.ui.*

class MainActivity : ComponentActivity() {
    override fun onPause() {
        (application as MaintApplication).container.temporary.paused()
        super.onPause()
    }
    override fun onResume() {
        super.onResume()
        // Les résultats de capture sont distribués avant la reprise de l’activité.
        (application as MaintApplication).container.temporary.resumed()
        (application as MaintApplication).container.refreshNetwork()
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val container = (application as MaintApplication).container
        setContent {
            val vm: MaintViewModel = viewModel(factory = object : ViewModelProvider.Factory {
                @Suppress("UNCHECKED_CAST")
                override fun <T : ViewModel> create(modelClass: Class<T>, extras: CreationExtras): T =
                    MaintViewModel(container, extras.createSavedStateHandle()) as T
            })
            val choice by vm.theme.collectAsStateWithLifecycle()
            val dark = choice?.isDark(isSystemInDarkTheme()) ?: true
            SideEffect {
                val transparent = Color.Transparent.toArgb()
                enableEdgeToEdge(
                    statusBarStyle = SystemBarStyle.auto(transparent, transparent) { dark },
                    navigationBarStyle = SystemBarStyle.auto(transparent, transparent) { dark },
                )
                if (android.os.Build.VERSION.SDK_INT >= 29) window.isNavigationBarContrastEnforced = false
            }
            ApexTheme(dark) { if (choice != null) MaintApp(vm) }
        }
    }
}
