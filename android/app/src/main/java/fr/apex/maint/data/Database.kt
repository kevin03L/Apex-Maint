package fr.apex.maint.data

import androidx.room.*
import kotlinx.coroutines.flow.Flow

@Entity(tableName = "snapshot", primaryKeys = ["scope", "collection", "id"])
data class CacheRow(val scope: String, val collection: String, val id: String,
    val payload: String, val deletedAt: String? = null)

@Dao interface MaintDao {
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
