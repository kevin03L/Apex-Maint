package fr.apex.maint.data

import android.util.Log
import fr.apex.maint.BuildConfig

/** Ni messages d'exception, ni URL, URI, noms, corps, cookies ou identifiants métier. */
fun debugFailure(stage: String, failure: Throwable) {
    if (!BuildConfig.DEBUG) return
    val causes = generateSequence(failure) { it.cause }.take(8).joinToString(" <- ") { cause ->
        cause.javaClass.simpleName + (if (cause is ApiFailure) " HTTP ${cause.status}" else "") +
            cause.stackTrace.take(5).joinToString("", prefix = " [", postfix = "]") {
            " ${it.className}.${it.methodName}:${it.lineNumber}"
        }
    }
    Log.w("ApexMaint", "$stage : $causes")
}
