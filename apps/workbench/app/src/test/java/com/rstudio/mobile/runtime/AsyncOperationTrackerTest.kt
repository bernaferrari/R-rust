package com.rstudio.mobile.runtime

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AsyncOperationTrackerTest {
    @Test
    fun exposesRunningAndCancellingIdentityUntilAcknowledged() {
        val tracker = AsyncOperationTracker()

        assertEquals(OperationIdentity(), tracker.snapshot())
        assertNull(tracker.requestCancellation())

        assertEquals(OperationIdentity(7uL, 3, OperationPhase.RUNNING), tracker.start(3, 7uL))
        assertEquals(
            OperationIdentity(7uL, 3, OperationPhase.CANCELLING),
            tracker.requestCancellation(),
        )
        assertTrue(tracker.accepts(3, 7uL))
        assertTrue(tracker.complete(3, 7uL))
        assertEquals(OperationIdentity(), tracker.snapshot())
    }

    @Test
    fun staleCompletionCannotOverwriteRerun() {
        val tracker = AsyncOperationTracker()

        tracker.start(1, 10uL)
        assertTrue(tracker.complete(1, 10uL))
        tracker.start(1, 11uL)

        assertFalse(tracker.accepts(1, 10uL))
        assertFalse(tracker.complete(1, 10uL))
        assertEquals(OperationIdentity(11uL, 1, OperationPhase.RUNNING), tracker.snapshot())
        assertTrue(tracker.accepts(1, 11uL))
    }

    @Test
    fun staleSessionCannotMatchReusedOperationId() {
        val tracker = AsyncOperationTracker()

        tracker.start(2, 0uL)

        assertFalse(tracker.accepts(1, 0uL))
        assertFalse(tracker.complete(1, 0uL))
        assertEquals(OperationIdentity(0uL, 2, OperationPhase.RUNNING), tracker.snapshot())
    }
}
