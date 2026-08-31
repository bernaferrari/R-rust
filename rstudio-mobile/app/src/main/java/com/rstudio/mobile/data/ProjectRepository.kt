package com.rstudio.mobile.data

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.security.MessageDigest

data class ProjectFile(
    val uri: String,
    val name: String,
    val relativePath: String,
    val localPath: String,
    val mimeType: String?,
    val isDirectory: Boolean,
    val size: Long,
)

data class WorkspaceProject(
    val name: String,
    val treeUri: String,
    val localRoot: String,
    val files: List<ProjectFile>,
)

data class RecoveredDocument(
    val name: String,
    val sourceUri: String?,
    val code: String,
)

data class CreatedProjectFile(val file: ProjectFile, val project: WorkspaceProject)

data class ImportedFile(val file: File, val project: WorkspaceProject?)

class ProjectRepository(private val context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun openProject(treeUri: Uri): WorkspaceProject {
        retainAccess(treeUri, write = true)
        preferences.edit().putString(KEY_PROJECT_URI, treeUri.toString()).apply()
        return loadProject(treeUri)
    }

    fun restoreProject(): WorkspaceProject? {
        val uri = preferences.getString(KEY_PROJECT_URI, null)?.let(Uri::parse) ?: return null
        return loadProject(uri)
    }

    fun clearProject() {
        preferences.edit().remove(KEY_PROJECT_URI).apply()
    }

    fun readText(file: ProjectFile): String =
        context.contentResolver.openInputStream(Uri.parse(file.uri)).use { input ->
            requireNotNull(input) { "Could not open ${file.name}" }
            input.bufferedReader(Charsets.UTF_8).readText()
        }

    fun readText(uri: Uri): String {
        retainAccess(uri, write = true)
        return context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open the selected document" }
            input.bufferedReader(Charsets.UTF_8).readText()
        }
    }

    fun createScript(project: WorkspaceProject, requestedName: String, code: String): CreatedProjectFile {
        val root = requireNotNull(DocumentFile.fromTreeUri(context, Uri.parse(project.treeUri))) {
            "The project folder is no longer available"
        }
        val base = requestedName.safeFileName().let { if (it.endsWith(".R", true)) it else "$it.R" }
        var name = base
        var suffix = 2
        while (root.findFile(name) != null) {
            name = "${base.substringBeforeLast('.')}-${suffix}.R"
            suffix += 1
        }
        val document = requireNotNull(root.createFile("text/x-r-source", name)) { "Could not create $name" }
        try {
            writeText(document.uri, null, code, persistAccess = false)
        } catch (error: Throwable) {
            document.delete()
            throw error
        }
        val refreshed = loadProject(Uri.parse(project.treeUri))
        val created = requireNotNull(refreshed.files.firstOrNull { it.uri == document.uri.toString() }) {
            "Created file is missing after project reconciliation"
        }
        return CreatedProjectFile(created, refreshed)
    }

    fun createFolder(project: WorkspaceProject, requestedName: String): WorkspaceProject {
        val root = requireNotNull(DocumentFile.fromTreeUri(context, Uri.parse(project.treeUri))) {
            "The project folder is no longer available"
        }
        val name = requestedName.safeFileName()
        require(root.findFile(name) == null) { "$name already exists" }
        requireNotNull(root.createDirectory(name)) { "Could not create folder $name" }
        return loadProject(Uri.parse(project.treeUri))
    }

    fun rename(project: WorkspaceProject, uri: Uri, requestedName: String): WorkspaceProject {
        val document = requireNotNull(DocumentFile.fromSingleUri(context, uri)) { "File is no longer available" }
        require(document.renameTo(requestedName.safeFileName())) { "Could not rename ${document.name}" }
        return loadProject(Uri.parse(project.treeUri))
    }

    fun delete(project: WorkspaceProject, uri: Uri): WorkspaceProject {
        val document = requireNotNull(DocumentFile.fromSingleUri(context, uri)) { "File is no longer available" }
        require(document.delete()) { "Could not delete ${document.name}" }
        return loadProject(Uri.parse(project.treeUri))
    }

    fun writeText(uri: Uri, localPath: String?, code: String, persistAccess: Boolean = true) {
        if (persistAccess) retainAccess(uri, write = true)
        context.contentResolver.openOutputStream(uri, "wt").use { output ->
            requireNotNull(output) { "Could not save the selected document" }
            output.write(code.toByteArray(Charsets.UTF_8))
        }
        localPath?.let { File(it).apply { parentFile?.mkdirs(); writeText(code, Charsets.UTF_8) } }
    }

    fun importFile(uri: Uri, preferredName: String, project: WorkspaceProject?): ImportedFile {
        if (project != null) return importIntoProject(uri, preferredName, project)
        val root = File(context.filesDir, "imports").also(File::mkdirs)
        val destination = uniqueFile(root, preferredName.safeFileName())
        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Could not open selected file" }
            destination.outputStream().use { output -> copyWithLimit(input, output, MAX_PROJECT_FILE_BYTES) }
        }
        return ImportedFile(destination, null)
    }

    fun saveRecovery(name: String, sourceUri: String?, code: String) {
        val recovery = File(context.filesDir, RECOVERY_FILE).also { it.parentFile?.mkdirs() }
        recovery.writeText(code, Charsets.UTF_8)
        preferences.edit()
            .putString(KEY_RECOVERY_NAME, name)
            .putString(KEY_RECOVERY_URI, sourceUri)
            .apply()
    }

    fun restoreRecovery(): RecoveredDocument? {
        val recovery = File(context.filesDir, RECOVERY_FILE)
        if (!recovery.isFile) return null
        return RecoveredDocument(
            name = preferences.getString(KEY_RECOVERY_NAME, null) ?: "untitled.R",
            sourceUri = preferences.getString(KEY_RECOVERY_URI, null),
            code = recovery.readText(Charsets.UTF_8),
        )
    }

    fun clearRecovery() {
        File(context.filesDir, RECOVERY_FILE).delete()
        preferences.edit().remove(KEY_RECOVERY_NAME).remove(KEY_RECOVERY_URI).apply()
    }

    private fun loadProject(treeUri: Uri): WorkspaceProject {
        val rootDocument = requireNotNull(DocumentFile.fromTreeUri(context, treeUri)) {
            "The selected folder is no longer available"
        }
        val projectName = rootDocument.name?.takeIf(String::isNotBlank) ?: "Android project"
        val localRoot = File(context.filesDir, "projects/${stableId(treeUri.toString())}")
        val files = ProjectMirrorReconciler().rebuild(localRoot, mirrorEntries(rootDocument))
        return WorkspaceProject(
            name = projectName,
            treeUri = treeUri.toString(),
            localRoot = localRoot.absolutePath,
            files = files.sortedWith(compareByDescending<ProjectFile> { it.isDirectory }.thenBy { it.relativePath.lowercase() }),
        )
    }

    private fun mirrorEntries(
        document: DocumentFile,
        relativePath: String = "",
    ): Sequence<ProjectMirrorEntry> = sequence {
        document.listFiles().forEach { child ->
            val name = child.name?.safeFileName()?.takeIf(String::isNotBlank) ?: return@forEach
            val childRelative = if (relativePath.isBlank()) name else "$relativePath/$name"
            yield(ProjectMirrorEntry(
                uri = child.uri.toString(),
                displayName = child.name ?: name,
                relativePath = childRelative,
                mimeType = child.type,
                isDirectory = child.isDirectory,
                openInput = if (child.isDirectory) null else ({
                    requireNotNull(context.contentResolver.openInputStream(child.uri)) {
                        "Could not read $childRelative"
                    }
                }),
            ))
            if (child.isDirectory) yieldAll(mirrorEntries(child, childRelative))
        }
    }

    private fun importIntoProject(
        sourceUri: Uri,
        preferredName: String,
        project: WorkspaceProject,
    ): ImportedFile {
        val root = requireNotNull(DocumentFile.fromTreeUri(context, Uri.parse(project.treeUri))) {
            "The project folder is no longer available"
        }
        val requested = preferredName.safeFileName()
        var name = requested
        var suffix = 2
        while (root.findFile(name) != null) {
            val stem = requested.substringBeforeLast('.', requested)
            val extension = requested.substringAfterLast('.', "").let { if (it.isBlank()) "" else ".$it" }
            name = "$stem-$suffix$extension"
            suffix += 1
        }
        val mimeType = context.contentResolver.getType(sourceUri) ?: "application/octet-stream"
        val document = requireNotNull(root.createFile(mimeType, name)) { "Could not import $name" }
        try {
            context.contentResolver.openInputStream(sourceUri).use { input ->
                requireNotNull(input) { "Could not open selected file" }
                context.contentResolver.openOutputStream(document.uri, "wt").use { output ->
                    requireNotNull(output) { "Could not write imported project file" }
                    copyWithLimit(input, output, MAX_PROJECT_FILE_BYTES)
                }
            }
        } catch (error: Throwable) {
            document.delete()
            throw error
        }
        val refreshed = loadProject(Uri.parse(project.treeUri))
        val imported = requireNotNull(refreshed.files.firstOrNull { it.uri == document.uri.toString() }) {
            "Imported file is missing after project reconciliation"
        }
        return ImportedFile(File(imported.localPath), refreshed)
    }

    private fun retainAccess(uri: Uri, write: Boolean) {
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or
            (if (write) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
        val retained = runCatching { context.contentResolver.takePersistableUriPermission(uri, flags) }
        if (retained.isFailure) {
            val alreadyRetained = context.contentResolver.persistedUriPermissions.any { permission ->
                permission.uri == uri && permission.isReadPermission && (!write || permission.isWritePermission)
            }
            check(alreadyRetained) { "Persistent access to the selected document was not granted" }
        }
    }

    private fun uniqueFile(root: File, requestedName: String): File {
        var candidate = File(root, requestedName)
        var suffix = 2
        while (candidate.exists()) {
            candidate = File(root, "${requestedName.substringBeforeLast('.', requestedName)}-$suffix" +
                requestedName.substringAfterLast('.', "").let { if (it.isBlank()) "" else ".$it" })
            suffix += 1
        }
        return candidate
    }

    private fun stableId(value: String): String = MessageDigest.getInstance("SHA-256")
        .digest(value.toByteArray())
        .take(8)
        .joinToString("") { "%02x".format(it) }

    private fun String.safeFileName(): String =
        replace(Regex("[^A-Za-z0-9._ -]"), "_").trim().ifBlank { "untitled" }

    companion object {
        private const val PREFERENCES = "r_workbench_projects"
        private const val KEY_PROJECT_URI = "active_project_uri"
        private const val KEY_RECOVERY_NAME = "recovery_name"
        private const val KEY_RECOVERY_URI = "recovery_uri"
        private const val RECOVERY_FILE = "recovery/current.R"
        private const val MAX_PROJECT_FILE_BYTES = 64L * 1024L * 1024L
    }
}

private fun copyWithLimit(input: java.io.InputStream, output: java.io.OutputStream, limit: Long) {
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    var copied = 0L
    while (true) {
        val count = input.read(buffer)
        if (count < 0) return
        if (count == 0) continue
        copied += count
        require(copied <= limit) { "Imported file is larger than $limit bytes" }
        output.write(buffer, 0, count)
    }
}
