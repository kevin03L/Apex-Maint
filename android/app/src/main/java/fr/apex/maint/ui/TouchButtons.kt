package fr.apex.maint.ui

import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.BorderStroke
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import fr.apex.maint.ui.theme.ApexLayout
import fr.apex.maint.ui.theme.LocalApexTokens

// La hauteur reste libre quand un libellé passe sur plusieurs lignes à grande police.
@Composable internal fun ApexOutlinedButton(onClick: () -> Unit, modifier: Modifier = Modifier,
    enabled: Boolean = true, content: @Composable RowScope.() -> Unit) {
    OutlinedButton(onClick, modifier.heightIn(min = ApexLayout.touch), enabled = enabled,
        border = BorderStroke(1.dp, LocalApexTokens.current.line), content = content)
}

@Composable internal fun ApexTextButton(onClick: () -> Unit, modifier: Modifier = Modifier,
    enabled: Boolean = true, content: @Composable RowScope.() -> Unit) {
    TextButton(onClick, modifier.heightIn(min = ApexLayout.touch), enabled = enabled, content = content)
}
