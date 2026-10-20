package fr.apex.maint.data

import androidx.room.*
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

@Entity(tableName = "snapshot", primaryKeys = ["scope", "collection", "id"])
data class CacheRow(val scope: String, val collection: String, val id: String,
    val payload: String, val deletedAt: String? = null)

@Dao interface MaintDao {
    @Query("SELECT * FROM snapshot") suspend fun allCache(): List<CacheRow>
    @Query("SELECT * FROM operations") suspend fun allOperations(): List<Operation>
    @Query("SELECT * FROM snapshot WHERE scope = :scope AND collection = :collection AND deletedAt IS NULL")
    suspend fun rows(scope: String, collection: String): List<CacheRow>
    @Query("SELECT * FROM snapshot WHERE scope = :scope AND collection = :collection AND id = :id AND deletedAt IS NULL")
    suspend fun row(scope: String, collection: String, id: String): CacheRow?
    @Upsert suspend fun cache(rows: List<CacheRow>)
    @Upsert suspend fun operation(op: Operation)
    @Query("SELECT * FROM operations WHERE scope = :scope AND deletedAt IS NULL ORDER BY position, id")
    suspend fun operations(scope: String): List<Operation>
    @Query("SELECT * FROM operations WHERE scope = :scope AND deletedAt IS NULL ORDER BY position, id")
    fun watchOperations(scope: String): Flow<List<Operation>>
    @Query("SELECT COALESCE(MAX(position), 0) + 1 FROM operations WHERE scope = :scope")
    suspend fun nextPosition(scope: String): Long
    @Query("UPDATE snapshot SET deletedAt = :at WHERE scope = :scope AND collection = :collection")
    suspend fun retire(scope: String, collection: String, at: String)
    // Effacement physique réservé à la purge du cache privé de cet appareil.
    @Query("DELETE FROM snapshot") suspend fun purgeCache()
    @Query("DELETE FROM operations") suspend fun purgeQueue()
}
@Database(entities = [CacheRow::class, Operation::class], version = 1, exportSchema = true)
abstract class MaintDatabase : RoomDatabase() { abstract fun dao(): MaintDao }

/** Seuls les index techniques restent lisibles ; corps, erreurs et instantanés sont chiffrés. */
class EncryptedDao(private val raw: MaintDao, private val cipher: PayloadCipher) : MaintDao by raw {
    private fun context(row: CacheRow) = "cache:${row.scope}:${row.collection}:${row.id}"
    private fun context(op: Operation) = "operation:${op.scope}:${op.id}"
    override suspend fun cache(rows: List<CacheRow>) = raw.cache(rows.map {
        it.copy(payload = cipher.encrypt(context(it), it.payload))
    })
    override suspend fun rows(scope: String, collection: String) = raw.rows(scope, collection).map {
        it.copy(payload = cipher.decrypt(context(it), it.payload))
    }
    override suspend fun row(scope: String, collection: String, id: String) = raw.row(scope, collection, id)?.let {
        it.copy(payload = cipher.decrypt(context(it), it.payload))
    }
    override suspend fun operation(op: Operation) = raw.operation(op.copy(
        body = cipher.encrypt(context(op), op.body), error = op.error?.let { cipher.encrypt(context(op) + ":error", it) }))
    override suspend fun operations(scope: String) = raw.operations(scope).map { op -> op.copy(
        body = cipher.decrypt(context(op), op.body), error = op.error?.let { cipher.decrypt(context(op) + ":error", it) }) }
    override fun watchOperations(scope: String) = raw.watchOperations(scope).map { ops ->
        ops.map { op -> op.copy(body = cipher.decrypt(context(op), op.body),
            error = op.error?.let { cipher.decrypt(context(op) + ":error", it) }) }
    }
    suspend fun protectLegacy(): Boolean {
        val rows = raw.allCache().filter { !it.payload.startsWith("gcm1:") }
        val ops = raw.allOperations().filter { !it.body.startsWith("gcm1:") }
        cache(rows); ops.forEach { operation(it) }
        return rows.isNotEmpty() || ops.isNotEmpty()
    }
}
