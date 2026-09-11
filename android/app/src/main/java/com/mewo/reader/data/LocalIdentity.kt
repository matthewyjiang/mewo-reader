package com.mewo.reader.data

data class LocalProfile(
    val name: String = "",
    val handle: String = "",
) {
    val ready: Boolean
        get() = handle.isNotBlank()

    val displayName: String
        get() = name.ifBlank { handle.ifBlank { "You" } }
}

object LocalIdentity {
    const val HANDLE_MIN = 3
    const val HANDLE_MAX = 32
    const val NAME_MAX = 50

    fun normalizeHandle(raw: String): String = raw.trim().trimStart('@')

    fun nameError(raw: String): String? {
        val trimmed = raw.trim()
        if (trimmed.length <= NAME_MAX) return null
        return "Name is longer than $NAME_MAX characters (asked ${trimmed.length})."
    }

    fun handleError(raw: String): String? {
        val normalized = normalizeHandle(raw)
        if (normalized.isEmpty()) return null
        return parse("", raw).exceptionOrNull()?.message
    }

    fun parse(name: String, handle: String): Result<LocalProfile> {
        val trimmedName = name.trim()
        nameError(trimmedName)?.let { return Result.failure(IllegalArgumentException(it)) }
        val normalized = normalizeHandle(handle)
        if (normalized.isEmpty()) {
            return Result.failure(IllegalArgumentException("Pick a handle."))
        }
        if (normalized.length < HANDLE_MIN || normalized.length > HANDLE_MAX) {
            return Result.failure(
                IllegalArgumentException(
                    "Handle must be $HANDLE_MIN to $HANDLE_MAX characters (asked ${normalized.length}).",
                ),
            )
        }
        if (!normalized.all { it.isAsciiLetterOrDigit() || it == '_' }) {
            return Result.failure(
                IllegalArgumentException("Handle can only use letters, digits, and underscore."),
            )
        }
        return Result.success(LocalProfile(name = trimmedName, handle = normalized))
    }

    private fun Char.isAsciiLetterOrDigit(): Boolean {
        return this in 'a'..'z' || this in 'A'..'Z' || this in '0'..'9'
    }
}
