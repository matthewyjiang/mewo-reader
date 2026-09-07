package com.mewo.reader.ui.library

import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.LibraryRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class LibraryUiState(
    val books: List<BookRecord> = emptyList(),
    val busy: Boolean = false,
    val error: String? = null,
)

class LibraryViewModel(
    private val repository: LibraryRepository,
) : ViewModel() {
    private val _state = MutableStateFlow(LibraryUiState())
    val state: StateFlow<LibraryUiState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            repository.load()
            repository.library.collect { snap ->
                _state.update { it.copy(books = snap.books) }
            }
        }
    }

    fun import(uri: Uri) = runWork {
        repository.importFromUri(uri)
    }

    fun importSample() = runWork {
        repository.importSample()
    }

    fun delete(id: String) {
        viewModelScope.launch { repository.delete(id) }
    }

    fun dismissError() {
        _state.update { it.copy(error = null) }
    }

    private fun runWork(block: suspend () -> Unit) {
        viewModelScope.launch {
            _state.update { it.copy(busy = true, error = null) }
            runCatching { block() }
                .onFailure { err ->
                    _state.update {
                        it.copy(error = err.message ?: "That file did not open.")
                    }
                }
            _state.update { it.copy(busy = false) }
        }
    }

    companion object {
        fun factory(repository: LibraryRepository) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return LibraryViewModel(repository) as T
            }
        }
    }
}
