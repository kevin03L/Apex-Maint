package fr.apex.maint.data

import kotlinx.serialization.json.JsonObject

/** Aide locale ; le serveur contrôle de nouveau les droits, l’affectation et les préconditions. */
object TerrainRules {
    fun inputProblem(body: JsonObject): String? {
        body.text("title")?.trim()?.let {
            if (it.isBlank() || it.codePointCount(0, it.length) > 128 || it.any(Char::isISOControl))
                return "Le titre doit contenir entre 1 et 128 caractères valides."
        }
        for ((field, limit) in listOf("description" to 4096, "solution_report" to 16384)) {
            body.text(field)?.let { if (it.codePointCount(0, it.length) > limit)
                return if (field == "description") "La description est limitée à 4 096 caractères." else "Le compte rendu est limité à 16 384 caractères." }
        }
        if (body.toString().toByteArray(Charsets.UTF_8).size > 65536) return "Le formulaire dépasse la taille maximale de 64 Kio. Réduisez les textes."
        return null
    }
    fun completionProblem(item: JsonObject, report: String, children: List<JsonObject> = item.list("sub_interventions")): String? {
        inputProblem(obj("solution_report" to report.j()))?.let { return it }
        return when {
        item.text("status") !in setOf("open", "in_progress") -> "Cette intervention ne peut plus être clôturée."
        report.isBlank() -> "Renseignez le compte rendu avant de clôturer."
        item.list("technicians").none { it.text("duration_minutes") != null } -> "Enregistrez le temps d’au moins un technicien avant de clôturer, même s’il est nul."
        children.any { it.text("status") in setOf("open", "in_progress") } -> "Terminez les sous-interventions actives avant de clôturer."
        else -> null
        }
    }
}
