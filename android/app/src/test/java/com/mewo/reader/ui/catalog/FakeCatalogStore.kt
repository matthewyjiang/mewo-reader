package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogAccess
import com.mewo.reader.data.CatalogPage
import com.mewo.reader.data.CatalogStore
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.WorkDetail
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class FakeCatalogStore : CatalogStore {
    private val _access = MutableStateFlow(CatalogAccess.On)
    override val access: StateFlow<CatalogAccess> = _access.asStateFlow()

    fun setAccess(value: CatalogAccess) {
        _access.value = value
    }

    val pageCalls = mutableListOf<Pair<String, Int>>()
    var pageHandler: suspend (String, Int) -> CatalogPage = { _, _ -> CatalogPage() }

    val workCalls = mutableListOf<String>()
    var workHandler: suspend (String) -> WorkDetail = { WorkDetail(it) }

    val requestCalls = mutableListOf<String>()
    var requestHandler: suspend (CatalogWork) -> BookRequest = { error("request") }

    val cancels = mutableListOf<String>()
    var cancelHandler: suspend (String) -> Unit = {}

    val retries = mutableListOf<String>()
    var retryHandler: suspend (String) -> BookRequest = { error("retry") }

    override suspend fun refreshAccess() = Unit

    override suspend fun page(query: String, page: Int): CatalogPage {
        pageCalls += query to page
        return pageHandler(query, page)
    }

    override suspend fun work(workKey: String): WorkDetail {
        workCalls += workKey
        return workHandler(workKey)
    }

    override suspend fun request(work: CatalogWork): BookRequest {
        requestCalls += work.workKey
        return requestHandler(work)
    }

    override suspend fun cancel(id: String) {
        cancels += id
        cancelHandler(id)
    }

    override suspend fun retry(id: String): BookRequest {
        retries += id
        return retryHandler(id)
    }
}
