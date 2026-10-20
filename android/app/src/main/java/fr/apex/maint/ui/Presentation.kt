package fr.apex.maint.ui

import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

fun frenchDate(value: String?, zone: ZoneId = ZoneId.systemDefault()): String = value?.let {
    runCatching { OffsetDateTime.parse(it).atZoneSameInstant(zone)
        .format(DateTimeFormatter.ofPattern("d MMM yyyy 'à' HH:mm", Locale.FRENCH)) }.getOrNull()
} ?: "Non renseignée"

fun frenchDuration(minutes: Int): String {
    val total = minutes.coerceAtLeast(0)
    return if (total < 60) "$total min" else "${total / 60} h ${"%02d".format(Locale.FRENCH, total % 60)}"
}

fun operationCount(count: Int) = if (count == 1) "1 opération en attente" else "$count opérations en attente"
