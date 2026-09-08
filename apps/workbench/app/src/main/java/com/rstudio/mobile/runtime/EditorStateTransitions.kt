package com.rstudio.mobile.runtime

import com.rstudio.mobile.data.ProjectFile

internal data class EditorSaveSnapshot(
    val documentId: String,
    val code: String,
    val name: String,
    val sourceUri: String?,
    val localPath: String?,
)

internal data class RecoveryDraft(
    val name: String,
    val sourceUri: String?,
    val code: String,
)

internal fun restoredEditorState(
    documents: List<EditorDocument>,
    requestedActiveId: String,
    consoleHistory: List<String>,
    recovery: RecoveryDraft?,
): RStudioUiState {
    val restored = documents.ifEmpty { RStudioUiState().documents }
    val validActiveId = requestedActiveId.takeIf { id -> restored.any { it.id == id } }
        ?: restored.first().id
    var state = RStudioUiState(consoleHistory = consoleHistory)
        .activateEditorDocument(restored.first { it.id == validActiveId }, restored)
    if (recovery == null) return state

    val matching = state.documents.firstOrNull { document ->
        recovery.sourceUri?.let { it == document.sourceUri }
            ?: (document.sourceUri == null && document.name == recovery.name)
    }
    val recovered = EditorDocument(
        id = matching?.id ?: uniqueRecoveryId(recovery, state.documents),
        name = recovery.name,
        code = recovery.code,
        sourceUri = recovery.sourceUri,
        localPath = matching?.localPath,
        isDirty = true,
    )
    state = state.activateEditorDocument(recovered, state.documents.upsertDocument(recovered))
    return state.copy(status = "Recovered unsaved work")
}

internal fun RStudioUiState.openEditorDocument(
    document: EditorDocument,
    status: String,
): RStudioUiState = activateEditorDocument(
    document = document,
    documents = documents.upsertDocument(document),
).copy(status = status, errorMessage = null)

internal fun RStudioUiState.editActiveDocument(newCode: String): RStudioUiState {
    val active = documents.firstOrNull { it.id == activeDocumentId } ?: return this
    val edited = active.copy(code = newCode, isDirty = true)
    return copy(
        code = newCode,
        isDirty = true,
        documents = documents.upsertDocument(edited),
    )
}

internal fun RStudioUiState.activeSaveSnapshot(): EditorSaveSnapshot {
    val active = documents.firstOrNull { it.id == activeDocumentId }
        ?: EditorDocument(
            id = activeDocumentId,
            name = currentFileName,
            code = code,
            sourceUri = currentDocumentUri,
            localPath = currentScriptPath,
            isDirty = isDirty,
        )
    return EditorSaveSnapshot(
        documentId = active.id,
        code = active.code,
        name = active.name,
        sourceUri = active.sourceUri,
        localPath = active.localPath,
    )
}

internal fun RStudioUiState.completeDocumentSave(
    snapshot: EditorSaveSnapshot,
    savedName: String = snapshot.name,
    savedSourceUri: String? = snapshot.sourceUri,
    savedLocalPath: String? = snapshot.localPath,
    status: String = "Saved $savedName",
): RStudioUiState {
    val current = documents.firstOrNull { it.id == snapshot.documentId } ?: return this
    val savedCurrentRevision = current.code == snapshot.code
    val updated = current.copy(
        name = savedName,
        sourceUri = savedSourceUri,
        localPath = savedLocalPath,
        isDirty = if (savedCurrentRevision) false else current.isDirty,
    )
    val updatedDocuments = documents.upsertDocument(updated)
    if (activeDocumentId != snapshot.documentId) {
        return copy(documents = updatedDocuments, status = status, errorMessage = null)
    }
    return copy(
        code = updated.code,
        currentFileName = updated.name,
        currentScriptPath = updated.localPath,
        currentDocumentUri = updated.sourceUri,
        isDirty = updated.isDirty,
        documents = updatedDocuments,
        status = status,
        errorMessage = null,
    )
}

internal fun RStudioUiState.isCurrent(snapshot: EditorSaveSnapshot): Boolean =
    documents.firstOrNull { it.id == snapshot.documentId }?.code == snapshot.code

internal fun RStudioUiState.canClearRecovery(snapshot: EditorSaveSnapshot): Boolean =
    activeDocumentId == snapshot.documentId && isCurrent(snapshot)

internal fun RStudioUiState.reconcileProjectDocuments(files: List<ProjectFile>): RStudioUiState {
    val byUri = files.associateBy(ProjectFile::uri)
    val reconciled = documents.map { document ->
        val file = document.sourceUri?.let(byUri::get) ?: return@map document
        document.copy(name = file.name, localPath = file.localPath)
    }
    val active = reconciled.firstOrNull { it.id == activeDocumentId } ?: return copy(documents = reconciled)
    return copy(
        documents = reconciled,
        code = active.code,
        currentFileName = active.name,
        currentScriptPath = active.localPath,
        currentDocumentUri = active.sourceUri,
        isDirty = active.isDirty,
    )
}

private fun RStudioUiState.activateEditorDocument(
    document: EditorDocument,
    documents: List<EditorDocument>,
): RStudioUiState = copy(
    code = document.code,
    currentFileName = document.name,
    currentScriptPath = document.localPath,
    currentDocumentUri = document.sourceUri,
    isDirty = document.isDirty,
    documents = documents,
    activeDocumentId = document.id,
    errorMessage = null,
)

private fun List<EditorDocument>.upsertDocument(document: EditorDocument): List<EditorDocument> =
    map { if (it.id == document.id) document else it }
        .let { updated -> if (updated.any { it.id == document.id }) updated else updated + document }

private fun uniqueRecoveryId(
    recovery: RecoveryDraft,
    documents: List<EditorDocument>,
): String {
    val base = "recovery-${recovery.name}"
    if (documents.none { it.id == base }) return base
    return generateSequence(2) { it + 1 }
        .map { "$base-$it" }
        .first { candidate -> documents.none { it.id == candidate } }
}
