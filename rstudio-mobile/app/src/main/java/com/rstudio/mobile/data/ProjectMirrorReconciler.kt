package com.rstudio.mobile.data

import java.io.File
import java.io.InputStream

internal data class ProjectMirrorLimits(
    val maxEntries: Int = 5_000,
    val maxDepth: Int = 24,
    val maxFileBytes: Long = 64L * 1024L * 1024L,
    val maxTotalBytes: Long = 512L * 1024L * 1024L,
)

internal data class ProjectMirrorEntry(
    val uri: String,
    val displayName: String,
    val relativePath: String,
    val mimeType: String?,
    val isDirectory: Boolean,
    val openInput: (() -> InputStream)? = null,
)

internal class ProjectMirrorReconciler(
    private val limits: ProjectMirrorLimits = ProjectMirrorLimits(),
) {
    fun rebuild(targetRoot: File, entries: Sequence<ProjectMirrorEntry>): List<ProjectFile> {
        val parent = requireNotNull(targetRoot.parentFile) { "Project mirror has no parent directory" }
        parent.mkdirs()
        val backup = File(parent, ".${targetRoot.name}.backup")
        if (!targetRoot.exists() && backup.exists()) {
            check(backup.renameTo(targetRoot)) { "Could not recover the previous project mirror" }
        }
        val staging = File(parent, ".${targetRoot.name}.staging-${System.nanoTime()}")
        check(staging.mkdirs()) { "Could not create project staging directory" }

        val mirrored = ArrayList<ProjectFile>()
        val seenPaths = HashSet<String>()
        var entryCount = 0
        var totalBytes = 0L
        try {
            entries.forEach { entry ->
                entryCount += 1
                require(entryCount <= limits.maxEntries) {
                    "This project contains more than ${limits.maxEntries} entries"
                }
                val relativePath = validatedRelativePath(entry.relativePath, limits.maxDepth)
                require(seenPaths.add(relativePath)) { "Project paths collide at $relativePath" }
                val local = File(staging, relativePath)
                require(local.canonicalPath.startsWith(staging.canonicalPath + File.separator)) {
                    "Project path escapes its mirror: $relativePath"
                }

                val actualSize = if (entry.isDirectory) {
                    check(local.mkdirs() || local.isDirectory) { "Could not mirror $relativePath" }
                    0L
                } else {
                    local.parentFile?.mkdirs()
                    val input = requireNotNull(entry.openInput) { "Could not read $relativePath" }.invoke()
                    input.use { source ->
                        local.outputStream().use { destination ->
                            copyBounded(source, destination, relativePath) { copied ->
                                require(copied <= limits.maxFileBytes) {
                                    "$relativePath is larger than ${limits.maxFileBytes} bytes"
                                }
                                require(totalBytes + copied <= limits.maxTotalBytes) {
                                    "This project is larger than ${limits.maxTotalBytes} bytes"
                                }
                            }
                        }
                    }
                }
                totalBytes += actualSize
                mirrored += ProjectFile(
                    uri = entry.uri,
                    name = entry.displayName,
                    relativePath = relativePath,
                    localPath = File(targetRoot, relativePath).absolutePath,
                    mimeType = entry.mimeType,
                    isDirectory = entry.isDirectory,
                    size = actualSize,
                )
            }
            replaceMirror(targetRoot, staging, backup)
            return mirrored
        } catch (error: Throwable) {
            staging.deleteRecursively()
            throw error
        }
    }

    private fun copyBounded(
        source: InputStream,
        destination: java.io.OutputStream,
        relativePath: String,
        validate: (Long) -> Unit,
    ): Long {
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        var copied = 0L
        while (true) {
            val count = source.read(buffer)
            if (count < 0) break
            if (count == 0) continue
            copied += count
            validate(copied)
            destination.write(buffer, 0, count)
        }
        require(copied >= 0) { "Could not mirror $relativePath" }
        return copied
    }

    private fun replaceMirror(target: File, staging: File, backup: File) {
        if (backup.exists()) check(backup.deleteRecursively()) { "Could not clear an old project backup" }
        val hadTarget = target.exists()
        if (hadTarget) check(target.renameTo(backup)) { "Could not preserve the existing project mirror" }
        if (!staging.renameTo(target)) {
            if (hadTarget) check(backup.renameTo(target)) { "Could not restore the existing project mirror" }
            error("Could not activate the refreshed project mirror")
        }
        if (backup.exists()) check(backup.deleteRecursively()) { "Could not remove the old project mirror" }
    }
}

private fun validatedRelativePath(path: String, maxDepth: Int): String {
    require(path.isNotBlank() && !path.startsWith('/') && !path.startsWith('\\')) {
        "Project entry has an invalid path"
    }
    val segments = path.replace('\\', '/').split('/')
    require(segments.size <= maxDepth) { "Project nesting is deeper than $maxDepth levels" }
    require(segments.all { it.isNotBlank() && it != "." && it != ".." }) {
        "Project entry has an invalid path"
    }
    return segments.joinToString("/")
}
