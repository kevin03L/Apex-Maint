package fr.apex.maint

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.*
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import fr.apex.maint.ui.*

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val container = (application as MaintApplication).container
        setContent {
            val vm: MaintViewModel = viewModel(factory = object : ViewModelProvider.Factory {
                @Suppress("UNCHECKED_CAST")
                override fun <T : ViewModel> create(modelClass: Class<T>): T = MaintViewModel(container) as T
            })
            MaterialTheme(colorScheme = lightColorScheme(
                primary = Color(0xFF0284C7), onPrimary = Color.White,
                secondary = Color(0xFF6366F1), background = Color(0xFFEEF2F8),
                surface = Color(0xFFF8FAFD), onSurface = Color(0xFF0F1B2E),
                onBackground = Color(0xFF0F1B2E), error = Color(0xFFDC2626),
            )) { MaintApp(vm) }
        }
    }
}
