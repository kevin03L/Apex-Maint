package fr.apex.maint

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.*
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewmodel.CreationExtras
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
                override fun <T : ViewModel> create(modelClass: Class<T>, extras: CreationExtras): T =
                    MaintViewModel(container, extras.createSavedStateHandle()) as T
            })
            MaterialTheme(colorScheme = lightColorScheme(
                primary = Color(0xFF0369A1), onPrimary = Color.White,
                primaryContainer = Color(0xFFDDEFFC), onPrimaryContainer = Color(0xFF0C3656),
                secondary = Color(0xFF6366F1), background = Color(0xFFEEF2F8),
                secondaryContainer = Color(0xFFE0E7FF), onSecondaryContainer = Color(0xFF312E81),
                surface = Color(0xFFF8FAFD), onSurface = Color(0xFF0F1B2E),
                surfaceContainerLowest = Color.White, surfaceContainerLow = Color(0xFFF2F6FC),
                surfaceContainer = Color(0xFFEAF0F8), surfaceContainerHigh = Color(0xFFE2E8F0),
                surfaceContainerHighest = Color(0xFFD7E1ED),
                onSurfaceVariant = Color(0xFF475569), outline = Color(0xFF64748B),
                inverseSurface = Color(0xFF17375E), inverseOnSurface = Color.White,
                errorContainer = Color(0xFFFEE2E2), onErrorContainer = Color(0xFF7F1D1D),
                onBackground = Color(0xFF0F1B2E), error = Color(0xFFDC2626),
            )) { MaintApp(vm) }
        }
    }
}
