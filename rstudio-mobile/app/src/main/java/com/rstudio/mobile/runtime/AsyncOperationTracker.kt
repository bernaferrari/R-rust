package com.rstudio.mobile.runtime

enum class OperationPhase {
    IDLE,
    RUNNING,
    CANCELLING,
}

data class OperationIdentity(
    val id: ULong? = null,
    val generation: Long = 0,
    val phase: OperationPhase = OperationPhase.IDLE,
)

/**
 * Owns the app adapter's operation identity.
 *
 * Callbacks and terminal results may arrive after cancellation or session
 * replacement. Callers must cross this seam before mutating UI state so an
 * event for operation N can never complete or overwrite operation N + 1.
 */
internal class AsyncOperationTracker {
    private var identity = OperationIdentity()

    @Synchronized
    fun start(generation: Long, id: ULong): OperationIdentity {
        check(identity.phase == OperationPhase.IDLE) { "an operation is already active" }
        return OperationIdentity(id, generation, OperationPhase.RUNNING).also { identity = it }
    }

    @Synchronized
    fun requestCancellation(): OperationIdentity? {
        if (identity.phase == OperationPhase.IDLE) return null
        return identity.copy(phase = OperationPhase.CANCELLING).also { identity = it }
    }

    @Synchronized
    fun accepts(generation: Long, id: ULong): Boolean =
        identity.generation == generation && identity.id == id && identity.phase != OperationPhase.IDLE

    @Synchronized
    fun complete(generation: Long, id: ULong): Boolean {
        if (!accepts(generation, id)) return false
        identity = OperationIdentity()
        return true
    }

    @Synchronized
    fun reset(): OperationIdentity = OperationIdentity().also { identity = it }

    @Synchronized
    fun snapshot(): OperationIdentity = identity
}
