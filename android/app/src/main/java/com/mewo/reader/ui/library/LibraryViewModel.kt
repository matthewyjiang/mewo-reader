package com.mewo.reader.ui.library

import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibraryStore
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class LibraryUiState(
    val load: LibraryLoad = LibraryLoading,
    val busy: Boolean = false,
    val error: String? = null,
)

class LibraryViewModel(
    private val store: LibraryStore,
) : ViewModel() {
    private val _state = MutableStateFlow(LibraryUiState())
    val state: StateFlow<LibraryUiState> = _state.asStateFlow()
    private var refreshJob: Job? = null

    init {
        viewModelScope.launch {
            store.loadState.collect { load ->
                _state.update { it.copy(load = load) }
            }
        }
        refresh()
    }

    /** Same load the screen uses on launch. A pull while one is running waits on it. */
    fun refresh() {
        if (refreshJob?.isActive == true) return
        refreshJob = viewModelScope.launch {
            runCatching { store.load() }
        }
    }

    fun import(uri: Uri, hosted: Boolean) = runWork {
        val book = store.importFromUri(uri)
        if (hosted && !book.isSample && !book.shared) {
            _state.update {
                it.copy(
                    error = "${book.title} is private. Use Share on Home when you want others to read it.",
                )
            }
        }
    }

    fun importSample() = runWork {
        store.importSample()
    }

    fun delete(id: String) {
        viewModelScope.launch {
            runCatching { store.delete(id) }
                .onFailure { err ->
                    _state.update {
                        it.copy(error = err.message ?: "Could not remove that book.")
                    }
                }
        }
    }

    fun setShared(id: String, shared: Boolean) {
        viewModelScope.launch {
            _state.update { it.copy(error = null) }
            runCatching { store.setShared(id, shared) }
                .onFailure { err ->
                    _state.update {
                        it.copy(error = err.message ?: "Could not change sharing.")
                    }
                }
        }
    }

    fun dismissError() {
        _state.update { it.copy(error = null) }
    }

    fun dismissLoadError() {
        store.dismissLoadError()
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
        fun factory(store: LibraryStore) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return LibraryViewModel(store) as T
            }
        }
    }
}
