package fr.apex.maint.data

/** La révision persistée refuse aussi un ancien brouillon chiffré restauré depuis une autre pile d'onglet. */
fun draftContext(server: String?, scope: String?, key: String, revisions: Map<String, String>): String =
    "brouillon:$server:$scope:$key" + revisions[key.substringBefore(':')]?.let { ":$it" }.orEmpty()
